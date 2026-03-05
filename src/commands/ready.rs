use anyhow::Result;
use colored::Colorize;
use serde_json::json;

use crate::config::find_config;
use crate::gh::graphql;

pub fn run() -> Result<()> {
    let (config, _) = find_config()?;

    // Query issues via the project so we can check status field
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
                                    assignees(first: 3) { nodes { login } }
                                    blockedByIssues(first: 5) {
                                        nodes { state }
                                    }
                                }
                            }
                            fieldValues(first: 8) {
                                nodes {
                                    ... on ProjectV2ItemFieldSingleSelectValue {
                                        name
                                        field { ... on ProjectV2SingleSelectField { name } }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    "#;

    let mut cursor: Option<String> = None;
    let mut ready = Vec::new();

    loop {
        let data = graphql(query, json!({
            "owner": config.owner,
            "repo": config.repo,
            "number": config.project_number,
            "cursor": cursor,
        }))?;

        let items_node = &data["repository"]["projectV2"]["items"];
        let nodes = items_node["nodes"].as_array().cloned().unwrap_or_default();

        for item in nodes {
            let content = &item["content"];
            let state = content["state"].as_str().unwrap_or("");

            // Skip closed issues and non-issues (drafts)
            if state != "OPEN" || content["number"].is_null() {
                continue;
            }

            // Check project status — skip in_progress
            let mut item_status = String::new();
            for fv in item["fieldValues"]["nodes"].as_array().unwrap_or(&vec![]) {
                let field_name = fv["field"]["name"].as_str().unwrap_or("");
                if field_name.eq_ignore_ascii_case("status") {
                    item_status = fv["name"].as_str().unwrap_or("").to_string();
                }
            }

            if item_status.eq_ignore_ascii_case("in_progress") {
                continue;
            }

            // Check blockers
            let has_open_blocker = content["blockedByIssues"]["nodes"]
                .as_array()
                .map(|blockers| blockers.iter().any(|b| b["state"].as_str() == Some("OPEN")))
                .unwrap_or(false);

            if has_open_blocker {
                continue;
            }

            let number = content["number"].as_u64().unwrap_or(0);
            let title = content["title"].as_str().unwrap_or("?").to_string();
            let assignees: Vec<String> = content["assignees"]["nodes"]
                .as_array()
                .map(|a| a.iter().filter_map(|u| u["login"].as_str().map(String::from)).collect())
                .unwrap_or_default();
            ready.push((number, title, assignees));
        }

        let has_next = items_node["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false);
        if !has_next {
            break;
        }
        cursor = items_node["pageInfo"]["endCursor"].as_str().map(String::from);
    }

    if ready.is_empty() {
        println!("{}", "No ready issues.".dimmed());
        return Ok(());
    }

    println!("{} ready issues (unblocked, not in progress):", ready.len().to_string().green().bold());
    println!("{}", "─".repeat(60).dimmed());
    for (num, title, assignees) in ready {
        let trunc = if title.len() > 52 { format!("{}…", &title[..51]) } else { title };
        let assignee_str = if assignees.is_empty() {
            "unassigned".dimmed().to_string()
        } else {
            assignees.join(", ")
        };
        println!("  #{:<6} {:<54} {}", num, trunc, assignee_str.dimmed());
    }

    Ok(())
}
