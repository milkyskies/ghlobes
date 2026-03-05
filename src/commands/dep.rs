use anyhow::{Context, Result};
use colored::Colorize;
use serde_json::json;

use crate::config::find_config;
use crate::gh::graphql;

pub fn add(issue: u64, blocked_by: u64) -> Result<()> {
    let (config, _) = find_config()?;
    let (issue_id, blocker_id) = get_issue_ids(&config, issue, blocked_by)?;

    let mutation = r#"
        mutation($issueId: ID!, $blockerId: ID!) {
            issueCreateBlockedByRelation(input: { issueId: $issueId, blockedByIssueId: $blockerId }) {
                issue { number title }
                blockingIssue { number title }
            }
        }
    "#;

    let data = graphql(mutation, json!({
        "issueId": issue_id,
        "blockerId": blocker_id,
    }))?;

    let issue_num = data["issueCreateBlockedByRelation"]["issue"]["number"].as_u64().unwrap_or(issue);
    let blocker_num = data["issueCreateBlockedByRelation"]["blockingIssue"]["number"].as_u64().unwrap_or(blocked_by);
    println!("{} #{issue_num} is now blocked by #{blocker_num}", "✓".green());

    Ok(())
}

pub fn remove(issue: u64, blocked_by: u64) -> Result<()> {
    let (config, _) = find_config()?;
    let (issue_id, blocker_id) = get_issue_ids(&config, issue, blocked_by)?;

    let mutation = r#"
        mutation($issueId: ID!, $blockerId: ID!) {
            issueRemoveBlockedByRelation(input: { issueId: $issueId, blockedByIssueId: $blockerId }) {
                issue { number title }
            }
        }
    "#;

    graphql(mutation, json!({
        "issueId": issue_id,
        "blockerId": blocker_id,
    }))?;

    println!("{} Removed: #{issue} no longer blocked by #{blocked_by}", "✓".green());
    Ok(())
}

pub fn list(issue: u64) -> Result<()> {
    let (config, _) = find_config()?;

    let query = r#"
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                issue(number: $number) {
                    number title
                    blockedByIssues(first: 20) {
                        nodes { number title state }
                    }
                    blockingIssues(first: 20) {
                        nodes { number title state }
                    }
                }
            }
        }
    "#;

    let data = graphql(query, json!({
        "owner": config.owner,
        "repo": config.repo,
        "number": issue,
    }))?;

    let iss = &data["repository"]["issue"];
    if iss.is_null() {
        anyhow::bail!("Issue #{issue} not found");
    }

    println!("#{issue} — {}", iss["title"].as_str().unwrap_or("?").bold());

    let blocked_by = iss["blockedByIssues"]["nodes"].as_array().cloned().unwrap_or_default();
    let blocking = iss["blockingIssues"]["nodes"].as_array().cloned().unwrap_or_default();

    if blocked_by.is_empty() && blocking.is_empty() {
        println!("  No dependencies.");
        return Ok(());
    }

    if !blocked_by.is_empty() {
        println!("{}", "  Blocked by:".yellow());
        for dep in &blocked_by {
            let state = dep["state"].as_str().unwrap_or("?");
            let icon = if state == "OPEN" { "●".red() } else { "●".green() };
            println!("    {} #{} {}", icon, dep["number"], dep["title"].as_str().unwrap_or("?"));
        }
    }

    if !blocking.is_empty() {
        println!("{}", "  Blocking:".dimmed());
        for dep in &blocking {
            let state = dep["state"].as_str().unwrap_or("?");
            let icon = if state == "OPEN" { "●".yellow() } else { "●".green() };
            println!("    {} #{} {}", icon, dep["number"], dep["title"].as_str().unwrap_or("?"));
        }
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
