use anyhow::Result;
use colored::Colorize;
use serde_json::json;

use crate::config::find_config;
use crate::gh::graphql;

pub fn run() -> Result<()> {
    let (config, _) = find_config()?;

    let query = r#"
        query($owner: String!, $repo: String!, $cursor: String) {
            repository(owner: $owner, name: $repo) {
                issues(first: 50, after: $cursor, states: OPEN) {
                    pageInfo { hasNextPage endCursor }
                    nodes {
                        number title
                        assignees(first: 3) { nodes { login } }
                        blockedBy(first: 10) {
                            nodes { number title state }
                        }
                    }
                }
            }
        }
    "#;

    let mut cursor: Option<String> = None;
    let mut blocked = Vec::new();

    loop {
        let data = graphql(query, json!({
            "owner": config.owner,
            "repo": config.repo,
            "cursor": cursor,
        }))?;

        let issues_node = &data["repository"]["issues"];
        let nodes = issues_node["nodes"].as_array().cloned().unwrap_or_default();

        for issue in nodes {
            let open_blockers: Vec<_> = issue["blockedBy"]["nodes"]
                .as_array()
                .map(|blockers| {
                    blockers
                        .iter()
                        .filter(|b| b["state"].as_str() == Some("OPEN"))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();

            if !open_blockers.is_empty() {
                let number = issue["number"].as_u64().unwrap_or(0);
                let title = issue["title"].as_str().unwrap_or("?").to_string();
                blocked.push((number, title, open_blockers));
            }
        }

        let has_next = issues_node["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false);
        if !has_next {
            break;
        }
        cursor = issues_node["pageInfo"]["endCursor"].as_str().map(String::from);
    }

    if blocked.is_empty() {
        println!("{}", "No blocked issues.".dimmed());
        return Ok(());
    }

    println!("{} blocked issues:", blocked.len().to_string().red().bold());
    println!("{}", "─".repeat(60).dimmed());
    for (num, title, blockers) in blocked {
        let trunc = if title.len() > 50 { format!("{}…", &title[..49]) } else { title };
        println!("  {} #{:<6} {}", "●".red(), num, trunc);
        for b in blockers {
            println!(
                "      {} blocked by #{} {}",
                "↳".dimmed(),
                b["number"],
                b["title"].as_str().unwrap_or("?").dimmed()
            );
        }
    }

    Ok(())
}
