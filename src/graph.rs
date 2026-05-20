use std::collections::{HashMap, HashSet};

use anyhow::Result;
use serde_json::json;

use crate::config::Config;
use crate::gh::graphql;

#[derive(Debug, Clone)]
pub struct IssueNode {
    pub number: u64,
    pub title: String,
    pub priority: String,
    pub points: Option<f64>,
    pub status: String,
    pub assignees: Vec<String>,
    /// Issue numbers that this issue is blocked by
    pub blocked_by: Vec<u64>,
    /// Open sub-issue numbers (makes this an epic)
    pub sub_issues: Vec<u64>,
    /// True if this issue has any sub-issues (open or closed)
    pub is_epic: bool,
}

#[derive(Debug)]
pub struct IssueGraph {
    pub nodes: HashMap<u64, IssueNode>,
    /// blocked_by edge: issue -> set of issues it's blocked by
    pub blocked_by: HashMap<u64, HashSet<u64>>,
    /// blocking edge (reverse): issue -> set of issues it blocks
    pub blocking: HashMap<u64, HashSet<u64>>,
    /// child -> parent epic (for the nearest parent epic of an issue)
    pub parent_of: HashMap<u64, u64>,
}

impl IssueGraph {
    /// Fetch all open issues and build the dependency graph.
    pub fn fetch(config: &Config) -> Result<Self> {
        let query = r#"
            query($owner: String!, $repo: String!, $number: Int!, $cursor: String) {
                repository(owner: $owner, name: $repo) {
                    projectV2(number: $number) {
                        items(first: 50, after: $cursor) {
                            pageInfo { hasNextPage endCursor }
                            nodes {
                                content {
                                    ... on Issue {
                                        number title state
                                        assignees(first: 5) { nodes { login } }
                                        blockedBy(first: 20) {
                                            nodes { number state }
                                        }
                                        subIssues(first: 50) {
                                            nodes { number state }
                                        }
                                    }
                                }
                                fieldValues(first: 10) {
                                    nodes {
                                        ... on ProjectV2ItemFieldSingleSelectValue {
                                            name
                                            field { ... on ProjectV2SingleSelectField { name } }
                                        }
                                        ... on ProjectV2ItemFieldNumberValue {
                                            number
                                            field { ... on ProjectV2Field { name } }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        "#;

        let mut nodes = HashMap::new();
        let mut cursor: Option<String> = None;

        loop {
            let data = graphql(
                query,
                json!({
                    "owner": config.owner,
                    "repo": config.repo,
                    "number": config.project_number,
                    "cursor": cursor,
                }),
            )?;

            let items_node = &data["repository"]["projectV2"]["items"];
            let items = items_node["nodes"].as_array().cloned().unwrap_or_default();

            for item in items {
                let content = &item["content"];
                let state = content["state"].as_str().unwrap_or("");

                // Only include open issues
                if state != "OPEN" || content["number"].is_null() {
                    continue;
                }

                let number = content["number"].as_u64().unwrap_or(0);
                let title = content["title"].as_str().unwrap_or("?").to_string();

                let assignees: Vec<String> = content["assignees"]["nodes"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|u| u["login"].as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();

                // Only include open blockers that are also open issues
                let blocked_by: Vec<u64> = content["blockedBy"]["nodes"]
                    .as_array()
                    .map(|b| {
                        b.iter()
                            .filter(|x| x["state"].as_str() == Some("OPEN"))
                            .filter_map(|x| x["number"].as_u64())
                            .collect()
                    })
                    .unwrap_or_default();

                let all_subs: Vec<_> = content["subIssues"]["nodes"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let is_epic = !all_subs.is_empty();
                let sub_issues: Vec<u64> = all_subs
                    .iter()
                    .filter(|s| s["state"].as_str() == Some("OPEN"))
                    .filter_map(|s| s["number"].as_u64())
                    .collect();

                // Extract project fields
                let mut priority = String::new();
                let mut status = String::new();
                let mut points = None;

                for fv in item["fieldValues"]["nodes"].as_array().unwrap_or(&vec![]) {
                    let field_name = fv["field"]["name"].as_str().unwrap_or("");
                    if field_name.eq_ignore_ascii_case("status") {
                        status = fv["name"].as_str().unwrap_or("").to_string();
                    } else if field_name.eq_ignore_ascii_case("priority") {
                        priority = fv["name"].as_str().unwrap_or("").to_string();
                    } else if field_name.eq_ignore_ascii_case("points") {
                        points = fv["number"].as_f64();
                    }
                }

                nodes.insert(
                    number,
                    IssueNode {
                        number,
                        title,
                        priority,
                        points,
                        status,
                        assignees,
                        blocked_by,
                        sub_issues,
                        is_epic,
                    },
                );
            }

            let has_next = items_node["pageInfo"]["hasNextPage"]
                .as_bool()
                .unwrap_or(false);
            if !has_next {
                break;
            }
            cursor = items_node["pageInfo"]["endCursor"]
                .as_str()
                .map(String::from);
        }

        // Build adjacency maps (only for edges where both ends exist in our graph)
        let mut blocked_by_map: HashMap<u64, HashSet<u64>> = HashMap::new();
        let mut blocking_map: HashMap<u64, HashSet<u64>> = HashMap::new();
        let mut parent_of: HashMap<u64, u64> = HashMap::new();

        for (num, node) in &nodes {
            for &dep in &node.blocked_by {
                if nodes.contains_key(&dep) {
                    blocked_by_map.entry(*num).or_default().insert(dep);
                    blocking_map.entry(dep).or_default().insert(*num);
                }
            }
            if node.is_epic {
                for &sub in &node.sub_issues {
                    if nodes.contains_key(&sub) {
                        // Latest epic wins if there are multiple parents (rare)
                        parent_of.insert(sub, *num);
                    }
                }
            }
        }

        Ok(IssueGraph {
            nodes,
            blocked_by: blocked_by_map,
            blocking: blocking_map,
            parent_of,
        })
    }

    /// Find epics whose title contains `query` (case-insensitive substring match).
    /// An issue is considered an epic if it has any sub-issues.
    pub fn find_epics_by_name(&self, query: &str) -> Vec<u64> {
        let q = query.to_lowercase();
        let mut matches: Vec<u64> = self
            .nodes
            .values()
            .filter(|n| n.is_epic && n.title.to_lowercase().contains(&q))
            .map(|n| n.number)
            .collect();
        matches.sort();
        matches
    }

    /// Walk up the epic chain, returning all ancestor epics (nearest first).
    pub fn ancestor_epics(&self, number: u64) -> Vec<u64> {
        let mut result = Vec::new();
        let mut seen = HashSet::new();
        let mut current = number;
        while let Some(&parent) = self.parent_of.get(&current) {
            if !seen.insert(parent) {
                break;
            }
            result.push(parent);
            current = parent;
        }
        result
    }

    /// Compute the transitive set of sub-issues rooted at `number`, including
    /// `number` itself. Only includes open issues that exist in the graph.
    /// Returns None if the root issue doesn't exist in the graph.
    pub fn epic_scope(&self, number: u64) -> Option<HashSet<u64>> {
        if !self.nodes.contains_key(&number) {
            return None;
        }
        let mut scope = HashSet::new();
        self.collect_subs(number, &mut scope);
        Some(scope)
    }

    fn collect_subs(&self, number: u64, visited: &mut HashSet<u64>) {
        if !visited.insert(number) {
            return;
        }
        if let Some(node) = self.nodes.get(&number) {
            for &sub in &node.sub_issues {
                if self.nodes.contains_key(&sub) {
                    self.collect_subs(sub, visited);
                }
            }
        }
    }

    /// Get the effective weight of an issue.
    /// For epics: weight = max sub-chain weight among open sub-issues.
    /// For regular issues: weight = points (or 1 if no points).
    pub fn weight(&self, number: u64, use_points: bool) -> f64 {
        let node = match self.nodes.get(&number) {
            Some(n) => n,
            None => return 1.0,
        };

        if node.is_epic && !node.sub_issues.is_empty() {
            // Epic weight = longest chain through its open sub-issues
            node.sub_issues
                .iter()
                .map(|&sub| self.path_weight(sub, use_points))
                .fold(0.0_f64, f64::max)
        } else if use_points {
            node.points.unwrap_or(1.0)
        } else {
            1.0
        }
    }

    /// Compute the longest path weight starting from this node (going downstream via blocking).
    /// If `scope` is Some, only edges within the scope set are followed.
    /// Uses memoization via the provided cache.
    fn path_weight_memo(
        &self,
        number: u64,
        use_points: bool,
        scope: Option<&HashSet<u64>>,
        cache: &mut HashMap<u64, f64>,
        visiting: &mut HashSet<u64>,
    ) -> f64 {
        if let Some(s) = scope {
            if !s.contains(&number) {
                return 0.0;
            }
        }
        if let Some(&cached) = cache.get(&number) {
            return cached;
        }
        if visiting.contains(&number) {
            // Cycle detected — break it
            return 0.0;
        }
        visiting.insert(number);

        let self_weight = self.weight(number, use_points);
        let max_downstream = self
            .blocking
            .get(&number)
            .map(|dependents| {
                dependents
                    .iter()
                    .filter(|&&dep| scope.map(|s| s.contains(&dep)).unwrap_or(true))
                    .map(|&dep| self.path_weight_memo(dep, use_points, scope, cache, visiting))
                    .fold(0.0_f64, f64::max)
            })
            .unwrap_or(0.0);

        visiting.remove(&number);
        let total = self_weight + max_downstream;
        cache.insert(number, total);
        total
    }

    /// Compute path weight from a node (public API).
    pub fn path_weight(&self, number: u64, use_points: bool) -> f64 {
        let mut cache = HashMap::new();
        let mut visiting = HashSet::new();
        self.path_weight_memo(number, use_points, None, &mut cache, &mut visiting)
    }

    /// Find the critical path — the longest weighted chain through the graph.
    /// If `scope` is Some, only issues within the scope are considered.
    /// Returns the path as a vec of issue numbers (from start to end).
    pub fn critical_path(
        &self,
        use_points: bool,
        scope: Option<&HashSet<u64>>,
    ) -> (Vec<u64>, f64) {
        let mut cache = HashMap::new();
        let mut visiting = HashSet::new();

        // Compute path weights for all nodes (in scope, if provided)
        let numbers: Vec<u64> = match scope {
            Some(s) => s.iter().copied().collect(),
            None => self.nodes.keys().copied().collect(),
        };
        for &num in &numbers {
            self.path_weight_memo(num, use_points, scope, &mut cache, &mut visiting);
        }

        // Find the node with the highest path weight
        let start = match cache.iter().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()) {
            Some((&num, _)) => num,
            None => return (vec![], 0.0),
        };

        // Trace the path from start
        let mut path = vec![start];
        let mut current = start;
        loop {
            let next = self
                .blocking
                .get(&current)
                .and_then(|deps| {
                    deps.iter()
                        .filter(|d| cache.contains_key(d))
                        .filter(|d| scope.map(|s| s.contains(d)).unwrap_or(true))
                        .max_by(|a, b| {
                            cache
                                .get(a)
                                .unwrap()
                                .partial_cmp(cache.get(b).unwrap())
                                .unwrap()
                        })
                        .copied()
                });

            match next {
                Some(n) => {
                    path.push(n);
                    current = n;
                }
                None => break,
            }
        }

        let total = cache.get(&start).copied().unwrap_or(0.0);
        (path, total)
    }

    /// Count how many issues are transitively unblocked by completing this issue.
    /// If `scope` is Some, only count descendants within the scope.
    pub fn transitive_unblocks(&self, number: u64, scope: Option<&HashSet<u64>>) -> usize {
        let mut visited = HashSet::new();
        self.collect_descendants(number, scope, &mut visited);
        visited.remove(&number);
        visited.len()
    }

    fn collect_descendants(
        &self,
        number: u64,
        scope: Option<&HashSet<u64>>,
        visited: &mut HashSet<u64>,
    ) {
        if let Some(s) = scope {
            if !s.contains(&number) {
                return;
            }
        }
        if !visited.insert(number) {
            return;
        }
        if let Some(dependents) = self.blocking.get(&number) {
            for &dep in dependents {
                self.collect_descendants(dep, scope, visited);
            }
        }
    }

    /// Check if an issue is ready (unblocked, open, not in progress, not an epic).
    pub fn is_ready(&self, number: u64) -> bool {
        let node = match self.nodes.get(&number) {
            Some(n) => n,
            None => return false,
        };

        // Skip in progress, backlog, done
        if node.status.eq_ignore_ascii_case("in progress")
            || node.status.eq_ignore_ascii_case("backlog")
            || node.status.eq_ignore_ascii_case("done")
        {
            return false;
        }

        // Skip epics (they're containers, not claimable work)
        if node.is_epic && !node.sub_issues.is_empty() {
            return false;
        }

        // Check no open blockers (that exist in our graph)
        let has_open_blocker = self
            .blocked_by
            .get(&number)
            .map(|deps| !deps.is_empty())
            .unwrap_or(false);

        !has_open_blocker
    }

    /// Check if a node is on the critical path.
    pub fn is_on_critical_path(&self, number: u64, critical_path: &[u64]) -> bool {
        critical_path.contains(&number)
    }

    /// Check if two issues share a near-future descendant (within depth limit).
    pub fn shares_descendant(&self, a: u64, b: u64, depth: usize) -> bool {
        let desc_a = self.descendants_within(a, depth);
        let desc_b = self.descendants_within(b, depth);
        desc_a.intersection(&desc_b).next().is_some()
    }

    fn descendants_within(&self, number: u64, depth: usize) -> HashSet<u64> {
        let mut result = HashSet::new();
        self.collect_descendants_depth(number, depth, &mut result);
        result
    }

    fn collect_descendants_depth(
        &self,
        number: u64,
        depth: usize,
        visited: &mut HashSet<u64>,
    ) {
        if depth == 0 || !visited.insert(number) {
            return;
        }
        if let Some(dependents) = self.blocking.get(&number) {
            for &dep in dependents {
                self.collect_descendants_depth(dep, depth - 1, visited);
            }
        }
    }
}
