use std::collections::HashSet;

use anyhow::Result;
use colored::Colorize;

use crate::config::find_config;
use crate::graph::IssueGraph;
use crate::util::truncate;

pub fn run(number: u64, upstream: bool, downstream: bool) -> Result<()> {
    let (config, _) = find_config()?;
    let graph = IssueGraph::fetch(&config)?;

    if !graph.nodes.contains_key(&number) {
        anyhow::bail!("Issue #{number} not found in project (or already closed)");
    }

    let root = graph.nodes.get(&number).unwrap();
    println!(
        "{} #{} {}",
        "Deps for".bold(),
        number,
        root.title.bold()
    );
    println!("{}", "─".repeat(70).dimmed());

    // Default: show both
    let show_upstream = upstream || (!upstream && !downstream);
    let show_downstream = downstream || (!upstream && !downstream);

    if show_upstream {
        let mut visited = HashSet::new();
        let upstream_count = count_transitive(&graph, number, true, &mut HashSet::new());
        println!(
            "{} {}",
            "Upstream".cyan().bold(),
            format!("(blocking — {upstream_count} transitively)").dimmed()
        );
        let blockers: Vec<u64> = graph
            .blocked_by
            .get(&number)
            .map(|s| {
                let mut v: Vec<u64> = s.iter().copied().collect();
                v.sort();
                v
            })
            .unwrap_or_default();
        if blockers.is_empty() {
            println!("  {}", "(unblocked)".dimmed());
        } else {
            visited.insert(number);
            let len = blockers.len();
            for (i, b) in blockers.iter().enumerate() {
                render(&graph, *b, "", i == len - 1, true, &mut visited);
            }
        }

        if show_downstream {
            println!();
        }
    }

    if show_downstream {
        let mut visited = HashSet::new();
        let downstream_count = count_transitive(&graph, number, false, &mut HashSet::new());
        println!(
            "{} {}",
            "Downstream".cyan().bold(),
            format!("(unblocks — {downstream_count} transitively)").dimmed()
        );
        let blocking: Vec<u64> = graph
            .blocking
            .get(&number)
            .map(|s| {
                let mut v: Vec<u64> = s.iter().copied().collect();
                v.sort();
                v
            })
            .unwrap_or_default();
        if blocking.is_empty() {
            println!("  {}", "(no downstream)".dimmed());
        } else {
            visited.insert(number);
            let len = blocking.len();
            for (i, b) in blocking.iter().enumerate() {
                render(&graph, *b, "", i == len - 1, false, &mut visited);
            }
        }
    }

    Ok(())
}

fn render(
    graph: &IssueGraph,
    number: u64,
    prefix: &str,
    is_last: bool,
    upstream: bool,
    visited: &mut HashSet<u64>,
) {
    let connector = if is_last { "└── " } else { "├── " };
    let node = match graph.nodes.get(&number) {
        Some(n) => n,
        None => {
            println!("{prefix}{connector}#{number} {}", "(closed/missing)".dimmed());
            return;
        }
    };

    let cycle = !visited.insert(number);
    let icon = "○".yellow();
    let title = truncate(&node.title, 48);
    let cycle_tag = if cycle {
        format!("  {}", "(cycle)".red().dimmed())
    } else {
        String::new()
    };

    println!("{prefix}{connector}{icon} #{number} {title}{cycle_tag}");

    if cycle {
        return;
    }

    let new_prefix = format!("{prefix}{}", if is_last { "    " } else { "│   " });
    let next: Vec<u64> = if upstream {
        graph
            .blocked_by
            .get(&number)
            .map(|s| {
                let mut v: Vec<u64> = s.iter().copied().collect();
                v.sort();
                v
            })
            .unwrap_or_default()
    } else {
        graph
            .blocking
            .get(&number)
            .map(|s| {
                let mut v: Vec<u64> = s.iter().copied().collect();
                v.sort();
                v
            })
            .unwrap_or_default()
    };

    let len = next.len();
    for (i, n) in next.iter().enumerate() {
        render(graph, *n, &new_prefix, i == len - 1, upstream, visited);
    }
}

fn count_transitive(
    graph: &IssueGraph,
    number: u64,
    upstream: bool,
    visited: &mut HashSet<u64>,
) -> usize {
    walk(graph, number, upstream, visited);
    visited.remove(&number);
    visited.len()
}

fn walk(
    graph: &IssueGraph,
    number: u64,
    upstream: bool,
    visited: &mut HashSet<u64>,
) {
    if !visited.insert(number) {
        return;
    }
    let next = if upstream {
        graph.blocked_by.get(&number)
    } else {
        graph.blocking.get(&number)
    };
    if let Some(set) = next {
        for &n in set {
            walk(graph, n, upstream, visited);
        }
    }
}
