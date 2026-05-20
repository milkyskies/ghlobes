use anyhow::{Context, Result};
use colored::Colorize;
use serde_json::json;
use std::io::{self, Write};

use crate::config::{Config, write_config};
use crate::gh::{gh, graphql};

pub fn run(
    owner: Option<String>,
    repo: Option<String>,
    project_number: Option<u64>,
) -> Result<()> {
    let (owner, repo) = match (owner, repo) {
        (Some(o), Some(r)) => (o, r),
        _ => detect_owner_repo()?,
    };

    println!("Setting up ghlobes for {}/{}", owner.bold(), repo.bold());

    let project_number = match project_number {
        Some(n) => n,
        None => find_or_create_project(&owner, &repo)?,
    };

    println!("Fetching project fields for project #{project_number}...");

    let query = r#"
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                projectV2(number: $number) {
                    id
                    fields(first: 30) {
                        nodes {
                            ... on ProjectV2SingleSelectField {
                                id
                                name
                                options { id name color description }
                            }
                            ... on ProjectV2Field {
                                id
                                name
                                dataType
                            }
                        }
                    }
                }
            }
        }
    "#;

    let data = graphql(
        query,
        json!({ "owner": owner, "repo": repo, "number": project_number }),
    )?;
    let project = &data["repository"]["projectV2"];
    let project_id = project["id"].as_str().context("No project ID")?.to_string();

    let fields = project["fields"]["nodes"]
        .as_array()
        .context("No fields found on project")?;

    // Find or create Status field
    let status_field_id = match find_field(fields, "status") {
        Some(id) => {
            println!("  {} Found Status field", "✓".green());
            warn_missing_status_options(fields, &id);
            id
        }
        None => {
            println!("  {} No Status field found, creating...", "→".yellow());
            create_status_field(&project_id)?
        }
    };

    // Find or create Priority field
    let priority_field_id = match find_field(fields, "priority") {
        Some(id) => {
            println!("  {} Found Priority field", "✓".green());
            id
        }
        None => {
            println!("  {} No Priority field found, creating...", "→".yellow());
            create_priority_field(&project_id)?
        }
    };

    // Find or create Points field
    let points_field_id = match find_number_field(fields, "points") {
        Some(id) => {
            println!("  {} Found Points field", "✓".green());
            Some(id)
        }
        None => {
            println!("  {} No Points field found, creating...", "→".yellow());
            Some(create_points_field(&project_id)?)
        }
    };

    // Show current options
    print_field_options(fields, "status");
    print_field_options(fields, "priority");

    let config = Config {
        owner: owner.clone(),
        repo: repo.clone(),
        project_number,
        status_field_id,
        priority_field_id,
        points_field_id,
    };

    let cwd = std::env::current_dir()?;
    let config_path = cwd.join(".ghlobes.toml");
    write_config(&config, &config_path)?;
    println!("{} Wrote {}", "✓".green(), config_path.display());

    println!("{} ghlobes initialized for {owner}/{repo}", "✓".green());

    Ok(())
}

fn prompt(message: &str) -> Result<String> {
    print!("{message}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn find_or_create_project(owner: &str, repo: &str) -> Result<u64> {
    // Check for existing projects
    let query = r#"
        query($owner: String!, $repo: String!) {
            repository(owner: $owner, name: $repo) {
                projectsV2(first: 10) {
                    nodes { number title }
                }
            }
        }
    "#;
    let data = graphql(query, json!({ "owner": owner, "repo": repo }))?;
    let projects = data["repository"]["projectsV2"]["nodes"]
        .as_array()
        .context("Failed to query projects")?;

    if !projects.is_empty() {
        println!("\nExisting projects on {owner}/{repo}:");
        for p in projects {
            println!(
                "  #{} — {}",
                p["number"],
                p["title"].as_str().unwrap_or("?")
            );
        }

        let answer = prompt("\nUse an existing project? [Y/n] ")?;
        if answer.is_empty()
            || answer.eq_ignore_ascii_case("y")
            || answer.eq_ignore_ascii_case("yes")
        {
            if projects.len() == 1 {
                let n = projects[0]["number"]
                    .as_u64()
                    .context("Bad project number")?;
                println!("Using project #{n}");
                return Ok(n);
            }
            let num_str = prompt("Enter project number: ")?;
            let n: u64 = num_str.parse().context("Invalid project number")?;
            return Ok(n);
        }
    } else {
        println!("\nNo existing projects found on {owner}/{repo}.");
    }

    // Create a new project
    let answer = prompt("Create a new GitHub Project? [Y/n] ")?;
    if !answer.is_empty()
        && !answer.eq_ignore_ascii_case("y")
        && !answer.eq_ignore_ascii_case("yes")
    {
        anyhow::bail!("No project selected. Run `ghlobes init --project <number>` to specify one.");
    }

    let title = prompt(&format!("Project title [{repo}]: "))?;
    let title = if title.is_empty() {
        repo.to_string()
    } else {
        title
    };

    // Get the owner node ID (needed for createProjectV2)
    let owner_query = r#"
        query($owner: String!) {
            repositoryOwner(login: $owner) { id }
        }
    "#;
    let owner_data = graphql(owner_query, json!({ "owner": owner }))?;
    let owner_id = owner_data["repositoryOwner"]["id"]
        .as_str()
        .context("Could not find owner ID")?
        .to_string();

    let create_mutation = r#"
        mutation($ownerId: ID!, $title: String!) {
            createProjectV2(input: { ownerId: $ownerId, title: $title }) {
                projectV2 { number id }
            }
        }
    "#;
    let create_data = graphql(
        create_mutation,
        json!({
            "ownerId": owner_id,
            "title": title,
        }),
    )?;

    let project_number = create_data["createProjectV2"]["projectV2"]["number"]
        .as_u64()
        .context("Failed to create project")?;

    let project_id = create_data["createProjectV2"]["projectV2"]["id"]
        .as_str()
        .context("No project ID returned")?
        .to_string();

    println!(
        "{} Created project \"{}\" (#{project_number})",
        "✓".green(),
        title
    );

    // Link the project to the repo
    let repo_query = r#"
        query($owner: String!, $repo: String!) {
            repository(owner: $owner, name: $repo) { id }
        }
    "#;
    let repo_data = graphql(repo_query, json!({ "owner": owner, "repo": repo }))?;
    let repo_id = repo_data["repository"]["id"]
        .as_str()
        .context("Could not find repo ID")?
        .to_string();

    let link_mutation = r#"
        mutation($projectId: ID!, $repositoryId: ID!) {
            linkProjectV2ToRepository(input: { projectId: $projectId, repositoryId: $repositoryId }) {
                repository { name }
            }
        }
    "#;
    let _ = graphql(
        link_mutation,
        json!({
            "projectId": project_id,
            "repositoryId": repo_id,
        }),
    );

    Ok(project_number)
}

fn create_status_field(project_id: &str) -> Result<String> {
    let mutation = r#"
        mutation($projectId: ID!, $name: String!, $options: [ProjectV2SingleSelectFieldOptionInput!]!) {
            createProjectV2Field(input: {
                projectId: $projectId
                dataType: SINGLE_SELECT
                name: $name
                singleSelectOptions: $options
            }) {
                projectV2Field { ... on ProjectV2SingleSelectField { id } }
            }
        }
    "#;

    let data = graphql(
        mutation,
        json!({
            "projectId": project_id,
            "name": "Status",
            "options": [
                { "name": "Backlog", "color": "BLUE", "description": "" },
                { "name": "Todo", "color": "GREEN", "description": "This item hasn't been started" },
                { "name": "In Progress", "color": "YELLOW", "description": "This is actively being worked on" },
                { "name": "Done", "color": "PURPLE", "description": "This has been completed" },
            ],
        }),
    )?;

    let field_id = data["createProjectV2Field"]["projectV2Field"]["id"]
        .as_str()
        .context("Failed to create Status field")?
        .to_string();

    println!(
        "  {} Created Status field (Backlog, Todo, In Progress, Done)",
        "✓".green()
    );
    Ok(field_id)
}

fn create_priority_field(project_id: &str) -> Result<String> {
    let mutation = r#"
        mutation($projectId: ID!, $name: String!, $options: [ProjectV2SingleSelectFieldOptionInput!]!) {
            createProjectV2Field(input: {
                projectId: $projectId
                dataType: SINGLE_SELECT
                name: $name
                singleSelectOptions: $options
            }) {
                projectV2Field { ... on ProjectV2SingleSelectField { id } }
            }
        }
    "#;

    let data = graphql(
        mutation,
        json!({
            "projectId": project_id,
            "name": "Priority",
            "options": [
                { "name": "P0", "color": "RED", "description": "Critical" },
                { "name": "P1", "color": "ORANGE", "description": "High" },
                { "name": "P2", "color": "YELLOW", "description": "Medium" },
                { "name": "P3", "color": "GREEN", "description": "Low" },
                { "name": "P4", "color": "GRAY", "description": "Backlog" },
            ],
        }),
    )?;

    let field_id = data["createProjectV2Field"]["projectV2Field"]["id"]
        .as_str()
        .context("Failed to create Priority field")?
        .to_string();

    println!("  {} Created Priority field (P0–P4)", "✓".green());
    Ok(field_id)
}

fn find_field(fields: &[serde_json::Value], name: &str) -> Option<String> {
    fields
        .iter()
        .find(|f| {
            f["name"]
                .as_str()
                .map(|n| n.eq_ignore_ascii_case(name))
                .unwrap_or(false)
        })
        .and_then(|f| f["id"].as_str())
        .map(String::from)
}

fn find_number_field(fields: &[serde_json::Value], name: &str) -> Option<String> {
    fields
        .iter()
        .find(|f| {
            f["name"]
                .as_str()
                .map(|n| n.eq_ignore_ascii_case(name))
                .unwrap_or(false)
                && f["dataType"].as_str() == Some("NUMBER")
        })
        .and_then(|f| f["id"].as_str())
        .map(String::from)
}

fn create_points_field(project_id: &str) -> Result<String> {
    let mutation = r#"
        mutation($projectId: ID!, $name: String!) {
            createProjectV2Field(input: {
                projectId: $projectId
                dataType: NUMBER
                name: $name
            }) {
                projectV2Field { ... on ProjectV2Field { id } }
            }
        }
    "#;

    let data = graphql(
        mutation,
        json!({
            "projectId": project_id,
            "name": "Points",
        }),
    )?;

    let field_id = data["createProjectV2Field"]["projectV2Field"]["id"]
        .as_str()
        .context("Failed to create Points field")?
        .to_string();

    println!(
        "  {} Created Points field (use Fibonacci: 1, 2, 3, 5, 8, 13)",
        "✓".green()
    );
    Ok(field_id)
}

fn warn_missing_status_options(fields: &[serde_json::Value], field_id: &str) {
    let required = ["backlog", "todo", "in progress", "done"];

    let field = match fields.iter().find(|f| f["id"].as_str() == Some(field_id)) {
        Some(f) => f,
        None => return,
    };

    let existing_opts = field["options"].as_array().cloned().unwrap_or_default();
    let existing_names: Vec<String> = existing_opts
        .iter()
        .filter_map(|o| o["name"].as_str().map(|s| s.to_lowercase()))
        .collect();

    let missing: Vec<&&str> = required
        .iter()
        .filter(|name| !existing_names.contains(&name.to_lowercase()))
        .collect();

    if !missing.is_empty() {
        let names: Vec<&str> = missing.iter().map(|n| **n).collect();
        println!(
            "  {} Missing status options: {}. Add them manually in your GitHub Project settings.",
            "⚠".yellow(),
            names.join(", ")
        );
    }
}

fn print_field_options(fields: &[serde_json::Value], name: &str) {
    if let Some(field) = fields.iter().find(|f| {
        f["name"]
            .as_str()
            .map(|n| n.eq_ignore_ascii_case(name))
            .unwrap_or(false)
    }) {
        if let Some(opts) = field["options"].as_array() {
            let names: Vec<&str> = opts.iter().filter_map(|o| o["name"].as_str()).collect();
            println!(
                "    {} options: {}",
                name.to_ascii_uppercase(),
                names.join(", ")
            );
        }
    }
}

fn detect_owner_repo() -> Result<(String, String)> {
    if let Ok(out) = gh(&["repo", "view", "--json", "owner,name"]) {
        let json: serde_json::Value = serde_json::from_str(&out)?;
        let owner = json["owner"]["login"]
            .as_str()
            .context("No owner")?
            .to_string();
        let name = json["name"].as_str().context("No repo name")?.to_string();
        return Ok((owner, name));
    }

    println!(
        "{} Not in a GitHub repo. Specify the repo to track issues against.",
        "→".yellow()
    );
    let default_owner = gh(&["api", "user", "--jq", ".login"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let owner = loop {
        let prompt_msg = match &default_owner {
            Some(d) => format!("GitHub owner [{d}]: "),
            None => "GitHub owner: ".to_string(),
        };
        let input = prompt(&prompt_msg)?;
        if !input.is_empty() {
            break input;
        }
        if let Some(d) = &default_owner {
            break d.clone();
        }
    };

    let repo = loop {
        let input = prompt("GitHub repo name: ")?;
        if !input.is_empty() {
            break input;
        }
    };

    Ok((owner, repo))
}
