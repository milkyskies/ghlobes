use std::collections::HashMap;

use anyhow::Result;
use colored::Colorize;

use crate::config::find_config;
use crate::graph::IssueGraph;
use crate::util::truncate;

pub fn run(top: usize) -> Result<()> {
    let (config, _) = find_config()?;
    let graph = IssueGraph::fetch(&config)?;

    if graph.nodes.is_empty() {
        println!("{}", "No open issues.".dimmed());
        return Ok(());
    }

    // Top blockers: rank issues by how many other open issues are stuck on them.
    // Direct count: number of open issues whose blocked_by contains this issue.
    let mut blocker_counts: Vec<(u64, usize)> = graph
        .nodes
        .keys()
        .filter_map(|&num| {
            let direct = graph.blocking.get(&num).map(|s| s.len()).unwrap_or(0);
            if direct > 0 {
                Some((num, direct))
            } else {
                None
            }
        })
        .collect();
    blocker_counts.sort_by(|a, b| b.1.cmp(&a.1));

    println!("{}", "Top blockers (by direct issues blocked):".bold());
    println!("{}", "─".repeat(70).dimmed());
    if blocker_counts.is_empty() {
        println!("  {}", "No issues are blocking other work.".dimmed());
    } else {
        for (num, count) in blocker_counts.iter().take(top) {
            let node = graph.nodes.get(num).unwrap();
            let title = truncate(&node.title, 38);
            let status = if node.status.is_empty() {
                "Todo".dimmed().to_string()
            } else {
                node.status.dimmed().to_string()
            };
            let assignee = if node.assignees.is_empty() {
                "unassigned".dimmed().to_string()
            } else {
                node.assignees.join(", ").dimmed().to_string()
            };
            println!("  #{num:<5} {title:<40} blocks {count:>2}  {status}  {assignee}");
        }
    }

    // Per-epic stuck counts: for each epic, how many of its sub-issues (recursively)
    // are blocked.
    let mut epic_stuck: HashMap<u64, usize> = HashMap::new();
    for (&num, _node) in &graph.nodes {
        // Is this issue stuck (has open blockers)?
        let stuck = graph
            .blocked_by
            .get(&num)
            .map(|s| !s.is_empty())
            .unwrap_or(false);
        if !stuck {
            continue;
        }
        // Walk up its ancestor epics, increment count for each
        for ancestor in graph.ancestor_epics(num) {
            *epic_stuck.entry(ancestor).or_insert(0) += 1;
        }
    }

    let mut epic_list: Vec<(u64, usize)> = epic_stuck.into_iter().collect();
    epic_list.sort_by(|a, b| b.1.cmp(&a.1));

    if !epic_list.is_empty() {
        println!();
        println!("{}", "Stuck issues per epic:".bold());
        println!("{}", "─".repeat(70).dimmed());
        for (epic_num, count) in epic_list.iter().take(top) {
            let node = match graph.nodes.get(epic_num) {
                Some(n) => n,
                None => continue,
            };
            let title = truncate(&node.title, 50);
            println!(
                "  #{epic_num:<5} {title:<52} {} stuck",
                count.to_string().yellow().bold()
            );
        }
    }

    // Summary line
    println!();
    let total_stuck = graph
        .blocked_by
        .iter()
        .filter(|(_, set)| !set.is_empty())
        .count();
    let unblock_top: usize = blocker_counts.iter().take(3).map(|(_, c)| *c).sum();
    if !blocker_counts.is_empty() && unblock_top > 0 {
        println!(
            "{}",
            format!(
                "{total_stuck} issues stuck. Unsticking the top 3 blockers would unblock {unblock_top} issues."
            )
            .dimmed()
        );
    }

    Ok(())
}
