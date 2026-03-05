use anyhow::{Context, Result};
use colored::Colorize;
use serde_json::json;

use crate::config::find_config;
use crate::gh::{gh, graphql};

pub fn run(
    title: Option<String>,
    body: Option<String>,
    label: Vec<String>,
    assignee: Vec<String>,
    priority: Option<String>,
    status: Option<String>,
) -> Result<()> {
    let (config, _) = find_config()?;

    // Build gh issue create args — it handles interactive editor/prompts for us
    let repo_str = format!("{}/{}", config.owner, config.repo);
    let mut args = vec!["issue", "create", "--repo", &repo_str];

    let title_str;
    if let Some(ref t) = title {
        title_str = t.clone();
        args.extend(["--title", &title_str]);
    }

    let body_str;
    if let Some(ref b) = body {
        body_str = b.clone();
        args.extend(["--body", &body_str]);
    }

    let label_args: Vec<String> = label.iter().flat_map(|l| vec!["--label".to_string(), l.clone()]).collect();
    let label_refs: Vec<&str> = label_args.iter().map(String::as_str).collect();
    args.extend(label_refs.iter().copied());

    let assignee_args: Vec<String> = assignee.iter().flat_map(|a| vec!["--assignee".to_string(), a.clone()]).collect();
    let assignee_refs: Vec<&str> = assignee_args.iter().map(String::as_str).collect();
    args.extend(assignee_refs.iter().copied());

    // gh issue create outputs the issue URL
    let out = gh(&args)?;
    let url = out.trim();
    println!("{} Created {url}", "✓".green());

    // Extract issue number from URL (ends with /123)
    let issue_number: u64 = url
        .rsplit('/')
        .next()
        .and_then(|s| s.parse().ok())
        .context("Could not parse issue number from URL")?;

    // Add to project and set fields
    add_to_project_and_set_fields(&config, issue_number, priority.as_deref(), status.as_deref())?;

    Ok(())
}

pub fn add_to_project_and_set_fields(
    config: &crate::config::Config,
    issue_number: u64,
    priority: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    // Get issue node ID
    let issue_data = crate::gh::gh_json(&[
        "issue", "view", &issue_number.to_string(),
        "--repo", &format!("{}/{}", config.owner, config.repo),
        "--json", "id",
    ])?;
    let issue_node_id = issue_data["id"].as_str().context("No issue node ID")?.to_string();

    // Get project node ID
    let proj_query = r#"
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                projectV2(number: $number) { id }
            }
        }
    "#;
    let proj_data = graphql(proj_query, json!({
        "owner": config.owner, "repo": config.repo, "number": config.project_number
    }))?;
    let project_id = proj_data["repository"]["projectV2"]["id"]
        .as_str()
        .context("No project ID")?
        .to_string();

    // Add issue to project
    let add_mutation = r#"
        mutation($projectId: ID!, $contentId: ID!) {
            addProjectV2ItemById(input: { projectId: $projectId, contentId: $contentId }) {
                item { id }
            }
        }
    "#;
    let add_data = graphql(add_mutation, json!({
        "projectId": project_id,
        "contentId": issue_node_id,
    }))?;
    let item_id = add_data["addProjectV2ItemById"]["item"]["id"]
        .as_str()
        .context("Failed to add issue to project")?
        .to_string();

    println!("{} Added to project #{}", "✓".green(), config.project_number);

    // Set priority field
    if let Some(p) = priority {
        set_single_select_field(config, &project_id, &item_id, &config.priority_field_id, p, "Priority")?;
    }

    // Set status field
    if let Some(s) = status {
        set_single_select_field(config, &project_id, &item_id, &config.status_field_id, s, "Status")?;
    }

    Ok(())
}

fn set_single_select_field(
    config: &crate::config::Config,
    project_id: &str,
    item_id: &str,
    field_id: &str,
    value: &str,
    field_label: &str,
) -> Result<()> {
    // Resolve option ID for the value
    let fields_query = r#"
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                projectV2(number: $number) {
                    fields(first: 20) {
                        nodes {
                            ... on ProjectV2SingleSelectField {
                                id name
                                options { id name }
                            }
                        }
                    }
                }
            }
        }
    "#;
    let fields_data = graphql(fields_query, json!({
        "owner": config.owner, "repo": config.repo, "number": config.project_number
    }))?;

    let fields = fields_data["repository"]["projectV2"]["fields"]["nodes"]
        .as_array()
        .context("No fields")?;

    let field = fields.iter().find(|f| f["id"].as_str() == Some(field_id))
        .with_context(|| format!("{field_label} field not found in project"))?;

    let option_id = field["options"]
        .as_array()
        .context("No options")?
        .iter()
        .find(|o| o["name"].as_str().map(|n| n.eq_ignore_ascii_case(value)).unwrap_or(false))
        .with_context(|| format!("Option '{value}' not found for {field_label}"))?["id"]
        .as_str()
        .context("Option has no id")?
        .to_string();

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

    graphql(mutation, json!({
        "projectId": project_id,
        "itemId": item_id,
        "fieldId": field_id,
        "optionId": option_id,
    }))?;

    println!("{} Set {field_label} → {value}", "✓".green());
    Ok(())
}
