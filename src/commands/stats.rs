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
                open: issues(states: OPEN, first: 1) { totalCount }
                closed: issues(states: CLOSED, first: 1) { totalCount }
                issues(first: 50, after: $cursor, states: OPEN) {
                    pageInfo { hasNextPage endCursor }
                    nodes {
                        blockedByIssues(first: 1) {
                            nodes { state }
                        }
                    }
                }
            }
        }
    "#;

    let mut cursor: Option<String> = None;
    let mut total_open = 0u64;
    let mut total_closed = 0u64;
    let mut blocked_count = 0u64;
    let mut first = true;

    loop {
        let data = graphql(query, json!({
            "owner": config.owner,
            "repo": config.repo,
            "cursor": cursor,
        }))?;

        if first {
            total_open = data["repository"]["open"]["totalCount"].as_u64().unwrap_or(0);
            total_closed = data["repository"]["closed"]["totalCount"].as_u64().unwrap_or(0);
            first = false;
        }

        let issues_node = &data["repository"]["issues"];
        for issue in issues_node["nodes"].as_array().unwrap_or(&vec![]) {
            let has_open_blocker = issue["blockedByIssues"]["nodes"]
                .as_array()
                .map(|b| b.iter().any(|x| x["state"].as_str() == Some("OPEN")))
                .unwrap_or(false);
            if has_open_blocker {
                blocked_count += 1;
            }
        }

        let has_next = issues_node["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false);
        if !has_next {
            break;
        }
        cursor = issues_node["pageInfo"]["endCursor"].as_str().map(String::from);
    }

    let ready = total_open.saturating_sub(blocked_count);
    let total = total_open + total_closed;

    println!("{}", "Issue Stats".bold());
    println!("{}", "─".repeat(30).dimmed());
    println!("  {:<12} {}", "Total:", total);
    println!("  {:<12} {}", "Open:", total_open.to_string().yellow());
    println!("  {:<12} {}", "Closed:", total_closed.to_string().green());
    println!("  {:<12} {}", "Blocked:", blocked_count.to_string().red());
    println!("  {:<12} {}", "Ready:", ready.to_string().cyan().bold());

    Ok(())
}
