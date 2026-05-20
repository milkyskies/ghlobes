use anyhow::Result;
use colored::Colorize;

use crate::commands::close;
use crate::config::find_config;
use crate::graph::IssueGraph;
use crate::util::truncate;

pub fn run(number: u64, comment: Option<String>) -> Result<()> {
    // Snapshot the graph BEFORE closing so we can compute what was newly unblocked.
    let (config, _) = find_config()?;
    let before = IssueGraph::fetch(&config)?;

    // Look up the title for the closed-confirmation header
    let title = before
        .nodes
        .get(&number)
        .map(|n| n.title.clone())
        .unwrap_or_else(|| "?".to_string());

    // Compute what was directly waiting on this issue
    let directly_blocked: Vec<u64> = before
        .blocking
        .get(&number)
        .map(|s| s.iter().copied().collect())
        .unwrap_or_default();

    // Newly-unblocked: each direct dependent that was blocked ONLY by this issue
    // (i.e., its blocked_by set is exactly {number} or all-but-this are already closed/missing).
    let mut newly_unblocked: Vec<u64> = Vec::new();
    for &dep_num in &directly_blocked {
        let dep_blockers = before
            .blocked_by
            .get(&dep_num)
            .cloned()
            .unwrap_or_default();
        // Only blocker is this one — if so, this close unblocks it
        if dep_blockers.len() == 1 && dep_blockers.contains(&number) {
            newly_unblocked.push(dep_num);
        }
    }
    newly_unblocked.sort();

    // Now actually close
    close::run(number, comment)?;

    println!();
    println!(
        "{} #{} {}",
        "Closed".green().bold(),
        number,
        truncate(&title, 50)
    );
    println!("{}", "─".repeat(60).dimmed());

    if newly_unblocked.is_empty() && directly_blocked.is_empty() {
        println!("{}", "No issues were waiting on this one.".dimmed());
    } else if newly_unblocked.is_empty() {
        println!(
            "{}",
            format!(
                "{} issue{} still partially waiting (had other blockers):",
                directly_blocked.len(),
                if directly_blocked.len() == 1 { "" } else { "s" }
            )
            .dimmed()
        );
        for &n in &directly_blocked {
            if let Some(node) = before.nodes.get(&n) {
                let other_blockers: Vec<u64> = before
                    .blocked_by
                    .get(&n)
                    .map(|s| s.iter().copied().filter(|&x| x != number).collect())
                    .unwrap_or_default();
                let blockers_str = other_blockers
                    .iter()
                    .map(|b| format!("#{b}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                println!(
                    "  #{n:<5} {}  {}",
                    truncate(&node.title, 42),
                    format!("(still blocked by {blockers_str})").dimmed()
                );
            }
        }
    } else {
        println!(
            "{}",
            format!(
                "Newly unblocked ({}):",
                newly_unblocked.len()
            )
            .green()
            .bold()
        );
        for &n in &newly_unblocked {
            if let Some(node) = before.nodes.get(&n) {
                println!("  #{n:<5} {}", truncate(&node.title, 50));
            }
        }

        // If some directly blocked weren't fully unblocked, list them too
        let still_blocked: Vec<u64> = directly_blocked
            .iter()
            .copied()
            .filter(|n| !newly_unblocked.contains(n))
            .collect();
        if !still_blocked.is_empty() {
            println!();
            println!(
                "{}",
                format!(
                    "Still partially waiting ({}):",
                    still_blocked.len()
                )
                .dimmed()
            );
            for &n in &still_blocked {
                if let Some(node) = before.nodes.get(&n) {
                    let other_blockers: Vec<u64> = before
                        .blocked_by
                        .get(&n)
                        .map(|s| s.iter().copied().filter(|&x| x != number).collect())
                        .unwrap_or_default();
                    let blockers_str = other_blockers
                        .iter()
                        .map(|b| format!("#{b}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    println!(
                        "  #{n:<5} {}  {}",
                        truncate(&node.title, 42),
                        format!("(still blocked by {blockers_str})").dimmed()
                    );
                }
            }
        }
    }

    // Suggest follow-up
    println!();
    let parent_epic = before.parent_of.get(&number).copied();
    if let Some(epic) = parent_epic {
        if let Some(epic_node) = before.nodes.get(&epic) {
            let total_subs = epic_node.sub_issues.len();
            // -1 because this one just closed
            let remaining = total_subs.saturating_sub(1);
            println!(
                "{}",
                format!(
                    "Part of epic #{epic} {} ({remaining} subs still open)",
                    truncate(&epic_node.title, 36)
                )
                .dimmed()
            );
        }
    }
    println!(
        "{}  {}",
        "Next:".bold(),
        "glb next --diverse --reason".green()
    );

    Ok(())
}
