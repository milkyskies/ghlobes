use anyhow::Result;
use colored::Colorize;

use crate::config::find_config;
use crate::graph::IssueGraph;
use crate::util::truncate;

pub fn run(by_count: bool, top: usize, epic: Option<u64>, explain: bool) -> Result<()> {
    let (config, _) = find_config()?;
    let graph = IssueGraph::fetch(&config)?;

    if graph.nodes.is_empty() {
        println!("{}", "No open issues.".dimmed());
        return Ok(());
    }

    // Resolve epic scope if requested
    let scope = match epic {
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

    let use_points = !by_count;
    let (path, total_weight) = graph.critical_path(use_points, scope.as_ref());

    if path.is_empty() {
        println!("{}", "No dependency chains found.".dimmed());
        return Ok(());
    }

    // Header
    let unit = if use_points { "points" } else { "issues" };
    let display_weight = if use_points && total_weight.fract() == 0.0 {
        format!("{}", total_weight as i64)
    } else if use_points {
        format!("{total_weight:.1}")
    } else {
        format!("{}", path.len())
    };
    println!(
        "{}  {}  {}",
        "Critical Path".bold(),
        format!("{display_weight} {unit}").cyan().bold(),
        format!("{} issues", path.len()).dimmed()
    );
    println!("{}", "─".repeat(60).dimmed());

    for (i, &num) in path.iter().enumerate() {
        let node = match graph.nodes.get(&num) {
            Some(n) => n,
            None => continue,
        };

        let is_first = i == 0;
        let is_ready = graph.is_ready(num);

        // Icon
        let icon = if node.is_epic && !node.sub_issues.is_empty() {
            "┄".dimmed().to_string()
        } else if is_first && is_ready {
            "►".green().bold().to_string()
        } else {
            "○".yellow().to_string()
        };

        // Points display
        let pts = if use_points {
            let w = graph.weight(num, true);
            if w.fract() == 0.0 {
                format!("{}pts", w as i64)
            } else {
                format!("{w:.1}pts")
            }
        } else {
            String::new()
        };

        // Priority
        let pri = if node.priority.is_empty() {
            String::new()
        } else {
            let colored = match node.priority.as_str() {
                "P0" => node.priority.red().bold().to_string(),
                "P1" => node.priority.red().to_string(),
                "P2" => node.priority.yellow().to_string(),
                _ => node.priority.dimmed().to_string(),
            };
            colored
        };

        // Status tag for ready/in-progress
        let tag = if is_ready {
            " READY".green().bold().to_string()
        } else if node.status.eq_ignore_ascii_case("in progress") {
            " IN PROGRESS".blue().to_string()
        } else {
            String::new()
        };

        // Epic annotation
        let epic_note = if node.is_epic && !node.sub_issues.is_empty() {
            let total_subs = node.sub_issues.len();
            format!("  {}", format!("(epic, {total_subs} open subs)").dimmed())
        } else {
            String::new()
        };

        let title_trunc = truncate(&node.title, 38);

        println!(
            "  {icon} #{num:<5} {title_trunc:<40} {pri:<4} {pts}{tag}{epic_note}",
        );

        if explain {
            // Show direct dependents (what this path step unblocks for the next)
            let dependents: Vec<u64> = graph
                .blocking
                .get(&num)
                .map(|s| {
                    let mut v: Vec<u64> = s.iter().copied().collect();
                    v.sort();
                    v
                })
                .unwrap_or_default();
            let unblocks_total = graph.transitive_unblocks(num, None);
            if !dependents.is_empty() {
                let names: Vec<String> = dependents
                    .iter()
                    .take(3)
                    .filter_map(|d| graph.nodes.get(d).map(|n| format!("#{d} {}", truncate(&n.title, 22))))
                    .collect();
                let extra = if dependents.len() > 3 {
                    format!(", +{} more", dependents.len() - 3)
                } else {
                    String::new()
                };
                println!(
                    "          {} unblocks {} total · next: {}{extra}",
                    "↳".dimmed(),
                    unblocks_total,
                    names.join(", ")
                );
            }
        }

        // If epic, show its longest sub-chain inline
        if node.is_epic && !node.sub_issues.is_empty() {
            // Find which sub-issue continues the critical path
            let longest_sub = node
                .sub_issues
                .iter()
                .max_by(|&&a, &&b| {
                    graph
                        .path_weight(a, use_points)
                        .partial_cmp(&graph.path_weight(b, use_points))
                        .unwrap()
                })
                .copied();

            if let Some(sub_num) = longest_sub {
                if let Some(sub_node) = graph.nodes.get(&sub_num) {
                    let sub_ready = graph.is_ready(sub_num);
                    let sub_tag = if sub_ready {
                        " READY".green().bold().to_string()
                    } else {
                        String::new()
                    };
                    let sub_title = truncate(&sub_node.title, 34);
                    println!(
                        "    {} #{:<5} {}{}  {}",
                        "↳".dimmed(),
                        sub_num,
                        sub_title,
                        sub_tag,
                        "← longest remaining sub".dimmed()
                    );
                }
            }
        }
    }

    // High-leverage section
    println!();
    println!(
        "{}",
        format!("High-leverage (top {top}, most transitive unblocks):").bold()
    );
    println!("{}", "─".repeat(60).dimmed());

    let mut leverage: Vec<(u64, usize)> = graph
        .nodes
        .keys()
        .filter(|&&num| scope.as_ref().map(|s| s.contains(&num)).unwrap_or(true))
        .map(|&num| (num, graph.transitive_unblocks(num, scope.as_ref())))
        .filter(|&(_, count)| count > 0)
        .collect();
    leverage.sort_by(|a, b| b.1.cmp(&a.1));
    leverage.truncate(top);

    if leverage.is_empty() {
        println!("  {}", "No issues with downstream dependencies.".dimmed());
    } else {
        for (num, count) in leverage {
            let node = graph.nodes.get(&num).unwrap();
            let title_trunc = truncate(&node.title, 40);
            let ready_tag = if graph.is_ready(num) {
                " READY".green().bold().to_string()
            } else {
                String::new()
            };
            println!(
                "  #{:<5} {:<42} unblocks {:>2}{}",
                num, title_trunc, count, ready_tag,
            );
        }
    }

    Ok(())
}
