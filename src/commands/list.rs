use anyhow::Result;
use colored::Colorize;
use serde_json::json;

use crate::config::find_config;
use crate::gh::graphql;

pub fn run(
    status: Option<String>,
    priority: Option<String>,
    assignee: Option<String>,
) -> Result<()> {
    let (config, _) = find_config()?;

    // We fetch issues + their project field values in one query via projectItems
    let query = r#"
        query($owner: String!, $repo: String!, $number: Int!, $cursor: String) {
            repository(owner: $owner, name: $repo) {
                projectV2(number: $number) {
                    items(first: 50, after: $cursor) {
                        pageInfo { hasNextPage endCursor }
                        nodes {
                            id
                            content {
                                ... on Issue {
                                    number title state
                                    assignees(first: 3) { nodes { login } }
                                    labels(first: 5) { nodes { name } }
                                }
                            }
                            fieldValues(first: 10) {
                                nodes {
                                    ... on ProjectV2ItemFieldSingleSelectValue {
                                        name
                                        field { ... on ProjectV2SingleSelectField { name } }
                                    }
                                    ... on ProjectV2ItemFieldNumberValue {
                                        number
                                        field { ... on ProjectV2Field { name } }
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
    let mut issues = Vec::new();

    loop {
        let vars = json!({
            "owner": config.owner,
            "repo": config.repo,
            "number": config.project_number,
            "cursor": cursor,
        });
        let data = graphql(query, vars)?;
        let items_node = &data["repository"]["projectV2"]["items"];
        let nodes = items_node["nodes"].as_array().cloned().unwrap_or_default();
        issues.extend(nodes);

        let has_next = items_node["pageInfo"]["hasNextPage"]
            .as_bool()
            .unwrap_or(false);
        if !has_next {
            break;
        }
        cursor = items_node["pageInfo"]["endCursor"]
            .as_str()
            .map(String::from);
    }

    // Parse and filter
    let mut rows = Vec::new();
    for item in &issues {
        let content = &item["content"];
        let number = match content["number"].as_u64() {
            Some(n) => n,
            None => continue, // draft or non-issue
        };
        let title = content["title"].as_str().unwrap_or("?");
        let state = content["state"].as_str().unwrap_or("?");

        if state == "CLOSED" {
            continue; // default: skip closed
        }

        let assignees: Vec<&str> = content["assignees"]["nodes"]
            .as_array()
            .map(|a| a.iter().filter_map(|u| u["login"].as_str()).collect())
            .unwrap_or_default();

        let mut item_status = String::from("—");
        let mut item_priority = String::from("—");
        let mut item_points = String::from("—");
        for fv in item["fieldValues"]["nodes"].as_array().unwrap_or(&vec![]) {
            let field_name = fv["field"]["name"].as_str().unwrap_or("");
            if field_name.eq_ignore_ascii_case("status") {
                item_status = fv["name"].as_str().unwrap_or("").to_string();
            } else if field_name.eq_ignore_ascii_case("priority") {
                item_priority = fv["name"].as_str().unwrap_or("").to_string();
            } else if field_name.eq_ignore_ascii_case("points") {
                if let Some(n) = fv["number"].as_f64() {
                    item_points = if n.fract() == 0.0 {
                        format!("{}", n as i64)
                    } else {
                        format!("{n}")
                    };
                }
            }
        }

        // Apply filters
        if let Some(ref s) = status {
            if !item_status.eq_ignore_ascii_case(s) {
                continue;
            }
        }
        if let Some(ref p) = priority {
            if !item_priority.eq_ignore_ascii_case(p) {
                continue;
            }
        }
        if let Some(ref a) = assignee {
            if !assignees.iter().any(|u| u.eq_ignore_ascii_case(a)) {
                continue;
            }
        }

        rows.push((
            number,
            title.to_string(),
            item_status,
            item_priority,
            item_points,
            assignees.join(","),
        ));
    }

    if rows.is_empty() {
        println!("{}", "No issues found.".dimmed());
        return Ok(());
    }

    println!(
        "{:<6} {:<48} {:<14} {:<10} {:<5} {}",
        "#".bold(),
        "Title".bold(),
        "Status".bold(),
        "Priority".bold(),
        "Pts".bold(),
        "Assignee".bold()
    );
    println!("{}", "─".repeat(95).dimmed());

    for (num, title, status, priority, points, assignee) in rows {
        let trunc_title = if title.len() > 46 {
            format!("{}…", &title[..45])
        } else {
            title
        };
        let colored_priority = match priority.as_str() {
            "P0" => priority.red().bold().to_string(),
            "P1" => priority.red().to_string(),
            "P2" => priority.yellow().to_string(),
            "P3" | "P4" => priority.dimmed().to_string(),
            _ => priority,
        };
        println!(
            "{:<6} {:<48} {:<14} {:<10} {:<5} {}",
            num, trunc_title, status, colored_priority, points, assignee
        );
    }

    Ok(())
}
