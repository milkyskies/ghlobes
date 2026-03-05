use anyhow::{Context, Result};
use colored::Colorize;
use serde_json::json;

use crate::config::find_config;
use crate::gh::graphql;

pub fn add(parent: u64, child: u64) -> Result<()> {
    let (config, _) = find_config()?;
    let (parent_id, child_id) = get_issue_ids(&config, parent, child)?;

    let mutation = r#"
        mutation($issueId: ID!, $subIssueId: ID!) {
            addSubIssue(input: { issueId: $issueId, subIssueId: $subIssueId }) {
                issue { number }
                subIssue { number }
            }
        }
    "#;

    graphql(mutation, json!({
        "issueId": parent_id,
        "subIssueId": child_id,
    }))?;

    println!("{} #{child} is now a sub-issue of #{parent}", "✓".green());
    Ok(())
}

pub fn remove(parent: u64, child: u64) -> Result<()> {
    let (config, _) = find_config()?;
    let (parent_id, child_id) = get_issue_ids(&config, parent, child)?;

    let mutation = r#"
        mutation($issueId: ID!, $subIssueId: ID!) {
            removeSubIssue(input: { issueId: $issueId, subIssueId: $subIssueId }) {
                issue { number }
                subIssue { number }
            }
        }
    "#;

    graphql(mutation, json!({
        "issueId": parent_id,
        "subIssueId": child_id,
    }))?;

    println!("{} #{child} removed from #{parent}", "✓".green());
    Ok(())
}

pub fn list(parent: u64) -> Result<()> {
    let (config, _) = find_config()?;

    let query = r#"
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                issue(number: $number) {
                    number title
                    subIssues(first: 100) {
                        nodes { number title state }
                    }
                }
            }
        }
    "#;

    let data = graphql(query, json!({
        "owner": config.owner,
        "repo": config.repo,
        "number": parent,
    }))?;

    let issue = &data["repository"]["issue"];
    if issue.is_null() {
        anyhow::bail!("Issue #{parent} not found");
    }

    let subs = issue["subIssues"]["nodes"].as_array().cloned().unwrap_or_default();

    println!("#{parent} — {}", issue["title"].as_str().unwrap_or("?").bold());

    if subs.is_empty() {
        println!("  No sub-issues.");
        return Ok(());
    }

    let total = subs.len();
    let done = subs.iter().filter(|s| s["state"].as_str() == Some("CLOSED")).count();
    println!("{}", format!("  Sub-issues ({done}/{total} done):").dimmed());

    for sub in &subs {
        let state = sub["state"].as_str().unwrap_or("?");
        let icon = if state == "OPEN" { "○".yellow() } else { "✓".green() };
        println!("  {} #{} {}", icon, sub["number"], sub["title"].as_str().unwrap_or("?"));
    }

    Ok(())
}

fn get_issue_ids(config: &crate::config::Config, a: u64, b: u64) -> Result<(String, String)> {
    let query = r#"
        query($owner: String!, $repo: String!, $a: Int!, $b: Int!) {
            repository(owner: $owner, name: $repo) {
                a: issue(number: $a) { id }
                b: issue(number: $b) { id }
            }
        }
    "#;
    let data = graphql(query, json!({
        "owner": config.owner, "repo": config.repo, "a": a, "b": b
    }))?;

    let id_a = data["repository"]["a"]["id"].as_str()
        .with_context(|| format!("Issue #{a} not found"))?
        .to_string();
    let id_b = data["repository"]["b"]["id"].as_str()
        .with_context(|| format!("Issue #{b} not found"))?
        .to_string();

    Ok((id_a, id_b))
}
