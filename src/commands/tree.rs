use std::collections::HashSet;

use anyhow::Result;
use colored::Colorize;
use serde_json::json;

use crate::config::find_config;
use crate::gh::graphql;
use crate::util::truncate;

#[derive(Debug, Clone)]
struct TreeNode {
    number: u64,
    title: String,
    state: String,
    open_blockers: Vec<u64>,
    sub_issues: Vec<u64>,
}

pub fn run(number: u64) -> Result<()> {
    let (config, _) = find_config()?;

    // Walk the sub-issue tree starting from `number`. We BFS-fetch each level
    // because GraphQL doesn't support arbitrary-depth recursion.
    let mut nodes: std::collections::HashMap<u64, TreeNode> = std::collections::HashMap::new();
    let mut to_fetch = vec![number];
    let mut fetched = HashSet::new();

    while !to_fetch.is_empty() {
        let batch: Vec<u64> = to_fetch.drain(..).collect();
        for num in batch {
            if !fetched.insert(num) {
                continue;
            }
            let node = fetch_one(&config.owner, &config.repo, num)?;
            for &sub in &node.sub_issues {
                if !fetched.contains(&sub) {
                    to_fetch.push(sub);
                }
            }
            nodes.insert(num, node);
        }
    }

    let root = nodes
        .get(&number)
        .ok_or_else(|| anyhow::anyhow!("Issue #{number} not found"))?
        .clone();

    // Compute progress for the root
    let (done, total) = count_progress(&nodes, &root);

    let header_state = match root.state.as_str() {
        "OPEN" => "Open".yellow().to_string(),
        "CLOSED" => "Closed".green().to_string(),
        _ => root.state.clone(),
    };
    let progress_pct = if total > 0 { (done * 100) / total } else { 0 };
    println!(
        "{} #{} {} {}",
        header_state,
        root.number,
        root.title.bold(),
        format!("({done}/{total} done, {progress_pct}%)").dimmed()
    );
    println!("{}", "─".repeat(70).dimmed());

    if root.sub_issues.is_empty() {
        println!("{}", "No sub-issues.".dimmed());
        return Ok(());
    }

    // Render the tree
    let subs = root.sub_issues.clone();
    let len = subs.len();
    for (i, sub) in subs.iter().enumerate() {
        let is_last = i == len - 1;
        render_node(&nodes, *sub, "", is_last);
    }

    Ok(())
}

fn render_node(
    nodes: &std::collections::HashMap<u64, TreeNode>,
    number: u64,
    prefix: &str,
    is_last: bool,
) {
    let node = match nodes.get(&number) {
        Some(n) => n,
        None => return,
    };

    let connector = if is_last { "└── " } else { "├── " };
    let icon = if node.state == "CLOSED" {
        "✓".green().to_string()
    } else {
        "○".yellow().to_string()
    };

    let title_trunc = truncate(&node.title, 50);
    let title_styled = if node.state == "CLOSED" {
        title_trunc.dimmed().to_string()
    } else {
        title_trunc
    };

    let mut tail = String::new();
    if node.state == "OPEN" {
        if node.open_blockers.is_empty() {
            tail = format!("  {}", "READY".green().bold());
        } else {
            let blockers_str = node
                .open_blockers
                .iter()
                .map(|b| format!("#{b}"))
                .collect::<Vec<_>>()
                .join(", ");
            tail = format!("  {}", format!("blocked by {blockers_str}").dimmed());
        }
    }

    println!("{prefix}{connector}{icon} #{number} {title_styled}{tail}");

    let new_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
    let len = node.sub_issues.len();
    for (i, sub) in node.sub_issues.iter().enumerate() {
        render_node(nodes, *sub, &new_prefix, i == len - 1);
    }
}

fn count_progress(
    nodes: &std::collections::HashMap<u64, TreeNode>,
    root: &TreeNode,
) -> (usize, usize) {
    if root.sub_issues.is_empty() {
        return (0, 0);
    }
    let mut done = 0;
    let mut total = 0;
    for sub in &root.sub_issues {
        total += 1;
        if let Some(n) = nodes.get(sub) {
            if n.state == "CLOSED" {
                done += 1;
            }
        }
    }
    (done, total)
}

fn fetch_one(owner: &str, repo: &str, number: u64) -> Result<TreeNode> {
    let query = r#"
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                issue(number: $number) {
                    number title state
                    blockedBy(first: 20) {
                        nodes { number state }
                    }
                    subIssues(first: 50) {
                        nodes { number }
                    }
                }
            }
        }
    "#;

    let data = graphql(
        query,
        json!({
            "owner": owner,
            "repo": repo,
            "number": number,
        }),
    )?;

    let issue = &data["repository"]["issue"];
    if issue.is_null() {
        anyhow::bail!("Issue #{number} not found");
    }

    let open_blockers: Vec<u64> = issue["blockedBy"]["nodes"]
        .as_array()
        .map(|b| {
            b.iter()
                .filter(|x| x["state"].as_str() == Some("OPEN"))
                .filter_map(|x| x["number"].as_u64())
                .collect()
        })
        .unwrap_or_default();

    let sub_issues: Vec<u64> = issue["subIssues"]["nodes"]
        .as_array()
        .map(|s| s.iter().filter_map(|x| x["number"].as_u64()).collect())
        .unwrap_or_default();

    Ok(TreeNode {
        number: issue["number"].as_u64().unwrap_or(0),
        title: issue["title"].as_str().unwrap_or("?").to_string(),
        state: issue["state"].as_str().unwrap_or("?").to_string(),
        open_blockers,
        sub_issues,
    })
}
