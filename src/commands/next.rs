use std::collections::HashSet;

use anyhow::Result;
use colored::Colorize;

use crate::config::find_config;
use crate::graph::{IssueGraph, IssueNode};
use crate::util::truncate;

pub struct NextOpts {
    pub agents: usize,
    pub epic: Option<u64>,
    pub track: Option<String>,
    pub diverse: bool,
    pub reason: bool,
    pub exclude: Vec<u64>,
}

pub fn run(opts: NextOpts) -> Result<()> {
    let (config, _) = find_config()?;
    let graph = IssueGraph::fetch(&config)?;

    if graph.nodes.is_empty() {
        println!("{}", "No open issues.".dimmed());
        return Ok(());
    }

    // Resolve --track to an epic number, then merge with --epic
    let resolved_epic = match (opts.epic, opts.track.as_deref()) {
        (Some(_), Some(_)) => {
            anyhow::bail!("Use --epic OR --track, not both");
        }
        (Some(n), None) => Some(n),
        (None, Some(name)) => {
            let matches = graph.find_epics_by_name(name);
            match matches.len() {
                0 => anyhow::bail!("No epic matched track name '{name}'"),
                1 => Some(matches[0]),
                _ => {
                    let mut msg = format!("Track '{name}' matched multiple epics:\n");
                    for m in &matches {
                        if let Some(node) = graph.nodes.get(m) {
                            msg.push_str(&format!("  #{m} {}\n", node.title));
                        }
                    }
                    msg.push_str("Use --epic <num> or refine the track name.");
                    anyhow::bail!(msg);
                }
            }
        }
        (None, None) => None,
    };

    // Resolve epic scope if requested
    let scope = match resolved_epic {
        Some(num) => match graph.epic_scope(num) {
            Some(s) => {
                let epic_node = graph.nodes.get(&num).unwrap();
                let title = &epic_node.title;
                println!(
                    "{} #{} {} {}",
                    "Scoped to epic".dimmed(),
                    num,
                    title.bold(),
                    format!("({} open issues in scope)", s.len()).dimmed()
                );
                println!();
                Some(s)
            }
            None => {
                anyhow::bail!("Epic #{num} not found (or has no open issues)");
            }
        },
        None => None,
    };

    let exclude: HashSet<u64> = opts.exclude.iter().copied().collect();

    // Get the critical path for bonus scoring (within scope if applicable)
    let (critical_path, _) = graph.critical_path(true, scope.as_ref());

    // Find all ready issues (within scope, not excluded)
    let mut candidates: Vec<(u64, f64)> = graph
        .nodes
        .keys()
        .filter(|&&num| graph.is_ready(num))
        .filter(|&&num| scope.as_ref().map(|s| s.contains(&num)).unwrap_or(true))
        .filter(|&&num| !exclude.contains(&num))
        .map(|&num| {
            let node = graph.nodes.get(&num).unwrap();
            let score = compute_score(node, &graph, &critical_path, scope.as_ref());
            (num, score)
        })
        .collect();

    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

    if candidates.is_empty() {
        println!("{}", "No ready issues to assign.".dimmed());
        println!(
            "{}",
            "All ready issues are blocked, in progress, in backlog, or excluded.".dimmed()
        );
        return Ok(());
    }

    // Greedy pick — anti-conflict + diverse rules
    let mut picked: Vec<u64> = Vec::new();
    let mut picked_epics: HashSet<u64> = HashSet::new();

    for &(num, _) in &candidates {
        if picked.len() >= opts.agents {
            break;
        }

        // Anti-conflict: don't pick two issues sharing a near-future descendant
        let conflicts = picked.iter().any(|&p| graph.shares_descendant(num, p, 3));
        if conflicts {
            continue;
        }

        // Diverse: don't pick two issues from the same parent epic
        if opts.diverse {
            let ancestors = graph.ancestor_epics(num);
            let nearest_epic = ancestors.first().copied();
            if let Some(epic) = nearest_epic {
                if picked_epics.contains(&epic) {
                    continue;
                }
                picked_epics.insert(epic);
            }
        }

        picked.push(num);
    }

    // Backfill if we couldn't fill all slots due to constraints
    if picked.len() < opts.agents {
        for &(num, _) in &candidates {
            if picked.len() >= opts.agents {
                break;
            }
            if !picked.contains(&num) {
                picked.push(num);
            }
        }
    }

    let actual = picked.len();
    let header = if opts.diverse {
        format!(
            "Next batch — {actual} agent{} (diverse epics)",
            if actual == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "Next batch — {actual} agent{}",
            if actual == 1 { "" } else { "s" }
        )
    };
    println!("{}", header.bold());
    println!("{}", "─".repeat(60).dimmed());

    for (i, &num) in picked.iter().enumerate() {
        let node = graph.nodes.get(&num).unwrap();
        let agent_num = i + 1;

        // Tags line
        let mut tags = Vec::new();
        if !node.priority.is_empty() {
            tags.push(match node.priority.as_str() {
                "P0" => node.priority.red().bold().to_string(),
                "P1" => node.priority.red().to_string(),
                "P2" => node.priority.yellow().to_string(),
                _ => node.priority.dimmed().to_string(),
            });
        }
        if let Some(pts) = node.points {
            tags.push(format_points(pts));
        }
        if graph.is_on_critical_path(num, &critical_path) {
            tags.push("critical path".red().bold().to_string());
        }
        let unblocks = graph.transitive_unblocks(num, scope.as_ref());
        if unblocks > 0 {
            tags.push(format!("unblocks {unblocks}"));
        } else {
            tags.push("independent".dimmed().to_string());
        }

        // Track label = nearest parent epic
        let track_label = graph.ancestor_epics(num).first().and_then(|&epic_num| {
            graph
                .nodes
                .get(&epic_num)
                .map(|n| (epic_num, truncate(&n.title, 24)))
        });

        let title_trunc = truncate(&node.title, 44);

        println!(
            "  {} {} {}",
            format!("Agent {agent_num}").cyan().bold(),
            "→".dimmed(),
            format!("glb update {num} --claim").green()
        );
        if let Some((epic_num, ref epic_title)) = track_label {
            println!(
                "            #{num:<5} {}  {}",
                title_trunc.bold(),
                format!("[#{epic_num} {epic_title}]").dimmed()
            );
        } else {
            println!("            #{num:<5} {}", title_trunc.bold());
        }
        println!("            {}", tags.join(" · "));

        if opts.reason {
            // Show what this pick actually unblocks (by name)
            let unblocked = collect_direct_dependents(&graph, num, scope.as_ref());
            if !unblocked.is_empty() {
                println!("            {}", "Unblocks directly:".dimmed());
                for dep_num in unblocked.iter().take(5) {
                    if let Some(dep) = graph.nodes.get(dep_num) {
                        let dep_title = truncate(&dep.title, 40);
                        println!("              {} #{dep_num} {dep_title}", "↳".dimmed());
                    }
                }
                if unblocked.len() > 5 {
                    println!(
                        "              {} {} more",
                        "↳".dimmed(),
                        unblocked.len() - 5
                    );
                }
            }
            // Show why it was picked
            let reason = explain_pick(node, &graph, &critical_path, scope.as_ref());
            println!("            {} {}", "Reason:".dimmed(), reason);
        }

        if i < picked.len() - 1 {
            println!();
        }
    }

    println!();
    println!(
        "{}",
        "Re-run `glb next` after these close — or use `glb done <num>`.".dimmed()
    );

    Ok(())
}

fn format_points(pts: f64) -> String {
    if pts.fract() == 0.0 {
        format!("{}pts", pts as i64)
    } else {
        format!("{pts:.1}pts")
    }
}

fn collect_direct_dependents(
    graph: &IssueGraph,
    number: u64,
    scope: Option<&HashSet<u64>>,
) -> Vec<u64> {
    let mut deps: Vec<u64> = graph
        .blocking
        .get(&number)
        .map(|s| {
            s.iter()
                .filter(|n| scope.map(|sc| sc.contains(n)).unwrap_or(true))
                .copied()
                .collect()
        })
        .unwrap_or_default();
    deps.sort();
    deps
}

fn compute_score(
    node: &IssueNode,
    graph: &IssueGraph,
    critical_path: &[u64],
    scope: Option<&HashSet<u64>>,
) -> f64 {
    let mut score = 0.0;

    // Priority weight
    score += match node.priority.as_str() {
        "P0" => 40.0,
        "P1" => 30.0,
        "P2" => 20.0,
        "P3" => 10.0,
        _ => 15.0, // No priority = between P2 and P3
    };

    // Direct transitive unblocks (leverage)
    let unblocks = graph.transitive_unblocks(node.number, scope);
    score += unblocks as f64 * 5.0;

    // Parent epic credit: closing this sub helps the epic close, which helps
    // unblock anything downstream of the epic. Walk all ancestor epics.
    for ancestor in graph.ancestor_epics(node.number) {
        let epic_unblocks = graph.transitive_unblocks(ancestor, scope);
        // Partial credit — the sub doesn't fully unblock the epic alone
        score += epic_unblocks as f64 * 2.0;
    }

    // Critical path bonus
    if critical_path.contains(&node.number) {
        score += 20.0;
    }

    // Smaller issues get a slight preference (quicker to unblock things)
    let pts = node.points.unwrap_or(3.0);
    if pts <= 2.0 {
        score += 5.0;
    }

    score
}

fn explain_pick(
    node: &IssueNode,
    graph: &IssueGraph,
    critical_path: &[u64],
    scope: Option<&HashSet<u64>>,
) -> String {
    let mut reasons = Vec::new();

    if critical_path.contains(&node.number) {
        reasons.push("on critical path".to_string());
    }

    let unblocks = graph.transitive_unblocks(node.number, scope);
    if unblocks >= 5 {
        reasons.push(format!("high leverage ({unblocks} downstream)"));
    } else if unblocks > 0 {
        reasons.push(format!("unblocks {unblocks}"));
    }

    // Epic credit
    let ancestors = graph.ancestor_epics(node.number);
    if let Some(&epic) = ancestors.first() {
        let epic_unblocks = graph.transitive_unblocks(epic, scope);
        if epic_unblocks > 0 {
            if let Some(epic_node) = graph.nodes.get(&epic) {
                reasons.push(format!(
                    "advances epic #{} (unblocks {})",
                    epic, epic_unblocks
                ));
                // Quiet the unused warning
                let _ = epic_node;
            }
        }
    }

    if matches!(node.priority.as_str(), "P0" | "P1") {
        reasons.push(format!("{} priority", node.priority));
    }

    let pts = node.points.unwrap_or(3.0);
    if pts <= 2.0 {
        reasons.push("small (quick win)".to_string());
    } else if pts >= 8.0 {
        reasons.push(format!("large effort ({}pts)", pts as i64));
    }

    if reasons.is_empty() {
        "available work".to_string()
    } else {
        reasons.join(", ")
    }
}
