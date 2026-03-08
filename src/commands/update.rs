use anyhow::{Context, Result};
use colored::Colorize;
use serde_json::json;

use crate::config::find_config;
use crate::gh::graphql;

pub fn run(
    number: u64,
    title: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    assignee: Option<String>,
    claim: bool,
    points: Option<f64>,
) -> Result<()> {
    let (config, _) = find_config()?;
    // --claim is shorthand for --status "In Progress"
    let status = if claim {
        Some("In Progress".to_string())
    } else {
        status
    };

    if title.is_none()
        && status.is_none()
        && priority.is_none()
        && assignee.is_none()
        && points.is_none()
    {
        anyhow::bail!(
            "Specify at least one of --title, --status, --priority, --assignee, --points, or --claim"
        );
    }

    if let Some(ref t) = title {
        crate::gh::gh(&[
            "issue",
            "edit",
            &number.to_string(),
            "--repo",
            &format!("{}/{}", config.owner, config.repo),
            "--title",
            t,
        ])?;
        println!("{} Title → {t}", "✓".green());
    }

    // If only title was updated, skip project field updates
    if status.is_none() && priority.is_none() && assignee.is_none() && points.is_none() {
        return Ok(());
    }

    // Get the project item ID for this issue
    let query = r#"
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                issue(number: $number) {
                    id
                    projectItems(first: 10) {
                        nodes {
                            id
                            project { number }
                        }
                    }
                }
            }
        }
    "#;

    let data = graphql(
        query,
        json!({
            "owner": config.owner,
            "repo": config.repo,
            "number": number,
        }),
    )?;

    let issue = &data["repository"]["issue"];
    if issue.is_null() {
        anyhow::bail!("Issue #{number} not found");
    }

    let item_id = issue["projectItems"]["nodes"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .find(|item| item["project"]["number"].as_u64() == Some(config.project_number))
        })
        .and_then(|item| item["id"].as_str())
        .map(String::from)
        .with_context(|| {
            format!(
                "Issue #{number} is not in project #{}",
                config.project_number
            )
        })?;

    let project_id = get_project_id(&config)?;

    if let Some(ref s) = status {
        set_single_select(
            &config,
            &project_id,
            &item_id,
            &config.status_field_id.clone(),
            s,
            "Status",
        )?;
    }

    if let Some(ref p) = priority {
        set_single_select(
            &config,
            &project_id,
            &item_id,
            &config.priority_field_id.clone(),
            p,
            "Priority",
        )?;
    }

    if let Some(ref a) = assignee {
        // Use gh CLI for assignee (simpler)
        crate::gh::gh(&[
            "issue",
            "edit",
            &number.to_string(),
            "--repo",
            &format!("{}/{}", config.owner, config.repo),
            "--add-assignee",
            a,
        ])?;
        println!("{} Assigned to {a}", "✓".green());
    }

    if let Some(p) = points {
        if let Some(ref field_id) = config.points_field_id {
            set_number_field(&project_id, &item_id, field_id, p)?;
        } else {
            eprintln!("Warning: no Points field configured. Run `glb init` to set it up.");
        }
    }

    Ok(())
}

fn get_project_id(config: &crate::config::Config) -> Result<String> {
    let query = r#"
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                projectV2(number: $number) { id }
            }
        }
    "#;
    let data = graphql(
        query,
        json!({
            "owner": config.owner, "repo": config.repo, "number": config.project_number
        }),
    )?;
    data["repository"]["projectV2"]["id"]
        .as_str()
        .map(String::from)
        .context("No project ID")
}

fn set_number_field(project_id: &str, item_id: &str, field_id: &str, value: f64) -> Result<()> {
    let mutation = r#"
        mutation($projectId: ID!, $itemId: ID!, $fieldId: ID!, $number: Float!) {
            updateProjectV2ItemFieldValue(input: {
                projectId: $projectId
                itemId: $itemId
                fieldId: $fieldId
                value: { number: $number }
            }) {
                projectV2Item { id }
            }
        }
    "#;

    graphql(
        mutation,
        json!({
            "projectId": project_id,
            "itemId": item_id,
            "fieldId": field_id,
            "number": value,
        }),
    )?;

    println!("{} Points → {value}", "✓".green());
    Ok(())
}

fn set_single_select(
    config: &crate::config::Config,
    project_id: &str,
    item_id: &str,
    field_id: &str,
    value: &str,
    field_label: &str,
) -> Result<()> {
    let option_id = resolve_option_id(config, field_id, value, field_label)?;

    let mutation = r#"
        mutation($projectId: ID!, $itemId: ID!, $fieldId: ID!, $optionId: String!) {
            updateProjectV2ItemFieldValue(input: {
                projectId: $projectId
                itemId: $itemId
                fieldId: $fieldId
                value: { singleSelectOptionId: $optionId }
            }) {
                projectV2Item { id }
            }
        }
    "#;

    graphql(
        mutation,
        json!({
            "projectId": project_id,
            "itemId": item_id,
            "fieldId": field_id,
            "optionId": option_id,
        }),
    )?;

    println!("{} {field_label} → {value}", "✓".green());
    Ok(())
}

fn resolve_option_id(
    config: &crate::config::Config,
    field_id: &str,
    value: &str,
    field_label: &str,
) -> Result<String> {
    let query = r#"
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                projectV2(number: $number) {
                    fields(first: 20) {
                        nodes {
                            ... on ProjectV2SingleSelectField {
                                id options { id name }
                            }
                        }
                    }
                }
            }
        }
    "#;
    let data = graphql(
        query,
        json!({
            "owner": config.owner, "repo": config.repo, "number": config.project_number
        }),
    )?;

    let fields = data["repository"]["projectV2"]["fields"]["nodes"]
        .as_array()
        .context("No fields")?;

    let field = fields
        .iter()
        .find(|f| f["id"].as_str() == Some(field_id))
        .with_context(|| format!("{field_label} field not found"))?;

    field["options"]
        .as_array()
        .context("No options")?
        .iter()
        .find(|o| {
            o["name"]
                .as_str()
                .map(|n| n.eq_ignore_ascii_case(value))
                .unwrap_or(false)
        })
        .with_context(|| format!("Option '{value}' not found for {field_label}"))?["id"]
        .as_str()
        .map(String::from)
        .context("Option has no id")
}
