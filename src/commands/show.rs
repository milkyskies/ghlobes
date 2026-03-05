use anyhow::Result;
use colored::Colorize;
use serde_json::json;

use crate::config::find_config;
use crate::gh::graphql;

pub fn run(number: u64) -> Result<()> {
    let (config, _) = find_config()?;

    let query = r#"
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                issue(number: $number) {
                    number title body state
                    createdAt updatedAt
                    author { login }
                    assignees(first: 5) { nodes { login } }
                    labels(first: 10) { nodes { name color } }
                    blockedBy(first: 10) { nodes { number title state } }
                    blocking(first: 10) { nodes { number title state } }
                    projectItems(first: 5) {
                        nodes {
                            project { number }
                            fieldValues(first: 10) {
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

    let data = graphql(query, json!({
        "owner": config.owner,
        "repo": config.repo,
        "number": number,
    }))?;

    let issue = &data["repository"]["issue"];
    if issue.is_null() {
        anyhow::bail!("Issue #{number} not found");
    }

    let title = issue["title"].as_str().unwrap_or("?");
    let state = issue["state"].as_str().unwrap_or("?");
    let author = issue["author"]["login"].as_str().unwrap_or("?");
    let body = issue["body"].as_str().unwrap_or("").trim();

    let state_colored = match state {
        "OPEN" => state.green().bold().to_string(),
        "CLOSED" => state.red().bold().to_string(),
        _ => state.to_string(),
    };

    println!("{} #{number}  {}", state_colored, title.bold());
    println!("{}", "─".repeat(70).dimmed());
    println!("Author:   {author}");

    let assignees: Vec<&str> = issue["assignees"]["nodes"]
        .as_array()
        .map(|a| a.iter().filter_map(|u| u["login"].as_str()).collect())
        .unwrap_or_default();
    if !assignees.is_empty() {
        println!("Assigned: {}", assignees.join(", "));
    }

    let labels: Vec<&str> = issue["labels"]["nodes"]
        .as_array()
        .map(|l| l.iter().filter_map(|lb| lb["name"].as_str()).collect())
        .unwrap_or_default();
    if !labels.is_empty() {
        println!("Labels:   {}", labels.join(", "));
    }

    // Project fields (status, priority) from the matching project
    let project_items = issue["projectItems"]["nodes"].as_array();
    if let Some(items) = project_items {
        for item in items {
            let proj_num = item["project"]["number"].as_u64().unwrap_or(0);
            if proj_num != config.project_number {
                continue;
            }
            for fv in item["fieldValues"]["nodes"].as_array().unwrap_or(&vec![]) {
                let field_name = fv["field"]["name"].as_str().unwrap_or("");
                let value = fv["name"].as_str().unwrap_or("");
                if field_name.eq_ignore_ascii_case("status") {
                    println!("Status:   {}", value.cyan());
                } else if field_name.eq_ignore_ascii_case("priority") {
                    let colored = match value {
                        "P0" => value.red().bold().to_string(),
                        "P1" => value.red().to_string(),
                        "P2" => value.yellow().to_string(),
                        _ => value.dimmed().to_string(),
                    };
                    println!("Priority: {colored}");
                }
            }
        }
    }

    // Dependencies
    let blocked_by = issue["blockedBy"]["nodes"].as_array().cloned().unwrap_or_default();
    let blocking = issue["blocking"]["nodes"].as_array().cloned().unwrap_or_default();

    if !blocked_by.is_empty() {
        println!("{}", "Blocked by:".yellow());
        for dep in &blocked_by {
            let state = dep["state"].as_str().unwrap_or("?");
            let icon = if state == "OPEN" { "●".red() } else { "●".green() };
            println!("  {} #{} {}", icon, dep["number"], dep["title"].as_str().unwrap_or("?"));
        }
    }
    if !blocking.is_empty() {
        println!("{}", "Blocking:".dimmed());
        for dep in &blocking {
            let state = dep["state"].as_str().unwrap_or("?");
            let icon = if state == "OPEN" { "●".yellow() } else { "●".green() };
            println!("  {} #{} {}", icon, dep["number"], dep["title"].as_str().unwrap_or("?"));
        }
    }

    if !body.is_empty() {
        println!();
        println!("{body}");
    }

    Ok(())
}
