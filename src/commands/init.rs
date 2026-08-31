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
    yes: bool,
) -> Result<()> {
    let (owner, repo) = match (owner, repo) {
        (Some(o), Some(r)) => (o, r),
        _ => detect_owner_repo(yes)?,
    };

    println!("Setting up ghlobes for {}/{}", owner.bold(), repo.bold());

    let project_number = match project_number {
        Some(n) => n,
        None => find_or_create_project(&owner, &repo, yes)?,
    };

    println!("Fetching project fields for project #{project_number}...");

    let (project_id, fields) = fetch_project_fields(&owner, &repo, project_number)?;
    let fields = fields.as_slice();

    // Find or create Status field
    let status_field_id = match find_field(fields, "status") {
        Some(id) => {
            println!("  {} Found Status field", "✓".green());
            repair_status_options(fields, &id)?;
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

    // Re-read the board: the summary below would otherwise report the snapshot taken before this run repaired or created anything.
    let (_, fields) = fetch_project_fields(&owner, &repo, project_number)?;
    print_field_options(&fields, "status");
    print_field_options(&fields, "priority");

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

fn fetch_project_fields(
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<(String, Vec<serde_json::Value>)> {
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
        json!({ "owner": owner, "repo": repo, "number": number }),
    )?;
    let project = &data["repository"]["projectV2"];
    let project_id = project["id"].as_str().context("No project ID")?.to_string();
    let fields = project["fields"]["nodes"]
        .as_array()
        .context("No fields found on project")?
        .clone();

    Ok((project_id, fields))
}

fn prompt(message: &str) -> Result<String> {
    print!("{message}");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

/// What `--yes` should do with the projects already linked to the repo.
///
/// One project is unambiguous, so it is reused. None means there is nothing to reuse and a fresh one is correct. Several is the only case a default cannot settle, and guessing there would silently attach the repo to the wrong board.
fn unattended_project_choice(existing: &[u64]) -> Result<Option<u64>> {
    match existing {
        [] => Ok(None),
        [only] => Ok(Some(*only)),
        _ => {
            let numbers: Vec<String> = existing.iter().map(|n| format!("#{n}")).collect();
            anyhow::bail!(
                "--yes cannot choose between {} projects ({}). Pass --project <number>.",
                existing.len(),
                numbers.join(", ")
            )
        }
    }
}

fn find_or_create_project(owner: &str, repo: &str, yes: bool) -> Result<u64> {
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

    if yes {
        let numbers: Vec<u64> = projects
            .iter()
            .filter_map(|p| p["number"].as_u64())
            .collect();

        if let Some(number) = unattended_project_choice(&numbers)? {
            println!("Using project #{number}");
            return Ok(number);
        }

        return create_project(owner, repo, repo.to_string());
    }

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

    create_project(owner, repo, title)
}

fn create_project(owner: &str, repo: &str, title: String) -> Result<u64> {
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

/// Workflow order, and the single source of truth for both field creation and repair of an existing field.
const STATUS_OPTIONS: [(&str, &str, &str); 6] = [
    ("Backlog", "BLUE", ""),
    ("Todo", "GREEN", "This item hasn't been started"),
    ("In Progress", "YELLOW", "This is actively being worked on"),
    (
        "Needs Decision",
        "ORANGE",
        "Waiting on a decision from you, whether an agent parked or you filed it undecided",
    ),
    (
        "In Review",
        "BLUE",
        "Draft PR open, waiting on human promotion and merge",
    ),
    ("Done", "PURPLE", "This has been completed"),
];

fn is_canonical(name: &str) -> bool {
    STATUS_OPTIONS
        .iter()
        .any(|(canonical, _, _)| canonical.eq_ignore_ascii_case(name))
}

fn option_name(option: &serde_json::Value) -> &str {
    option["name"].as_str().unwrap_or_default()
}

/// Merge the canonical options into what the board already has, in workflow order.
///
/// Existing entries keep their IDs, because `updateProjectV2Field` replaces the whole option list and dropping an ID would clear that status from every item currently assigned to it.
///
/// Canonical options are ordered relative to each other by `STATUS_OPTIONS`, so a board that GitHub seeded with Todo / In Progress / Done reads as work actually moves once the autopilot statuses arrive. A custom status the owner added is not reshuffled into that sequence: it stays anchored directly after whichever canonical option preceded it, so the arrangement its owner chose survives.
fn merge_status_options(existing: &[serde_json::Value]) -> (Vec<serde_json::Value>, Vec<String>) {
    let normalized: Vec<serde_json::Value> = existing
        .iter()
        .map(|o| {
            json!({
                "id": o["id"].as_str().unwrap_or_default(),
                "name": option_name(o),
                "color": o["color"].as_str().unwrap_or("GRAY"),
                "description": o["description"].as_str().unwrap_or_default(),
            })
        })
        .collect();

    let mut added = Vec::new();

    // Canonical options in workflow order, reusing the board's own entry wherever it already has one.
    let mut merged: Vec<serde_json::Value> = Vec::new();
    for (name, color, description) in STATUS_OPTIONS {
        match normalized
            .iter()
            .find(|o| option_name(o).eq_ignore_ascii_case(name))
        {
            Some(present) => merged.push(present.clone()),
            None => {
                merged.push(json!({
                    "name": name,
                    "color": color,
                    "description": description,
                }));
                added.push(name.to_string());
            }
        }
    }

    // Each custom status goes back directly after the canonical option it followed on the original board, so its owner's placement is preserved even though the canonical ones moved around it.
    let mut anchor: Option<String> = None;
    for option in &normalized {
        let name = option_name(option);

        if is_canonical(name) {
            anchor = Some(name.to_string());
            continue;
        }

        let at = match &anchor {
            Some(after) => merged
                .iter()
                .position(|o| option_name(o).eq_ignore_ascii_case(after))
                .map(|index| index + 1)
                .unwrap_or(merged.len()),
            None => 0,
        };

        merged.insert(at, option.clone());
        anchor = Some(name.to_string());
    }

    (merged, added)
}

/// Whether the board's option list already matches the merged one, name for name and in the same order.
fn options_match(existing: &[serde_json::Value], merged: &[serde_json::Value]) -> bool {
    existing.len() == merged.len()
        && existing
            .iter()
            .zip(merged)
            .all(|(a, b)| option_name(a).eq_ignore_ascii_case(option_name(b)))
}

fn repair_status_options(fields: &[serde_json::Value], field_id: &str) -> Result<()> {
    let field = match fields.iter().find(|f| f["id"].as_str() == Some(field_id)) {
        Some(f) => f,
        None => return Ok(()),
    };

    let existing = field["options"].as_array().cloned().unwrap_or_default();
    let (merged, added) = merge_status_options(&existing);

    // A board can hold every option and still be out of order, so completeness alone is not the test for whether there is work to do.
    if options_match(&existing, &merged) {
        return Ok(());
    }

    let mutation = r#"
        mutation($fieldId: ID!, $options: [ProjectV2SingleSelectFieldOptionInput!]!) {
            updateProjectV2Field(input: { fieldId: $fieldId, singleSelectOptions: $options }) {
                projectV2Field { ... on ProjectV2SingleSelectField { id } }
            }
        }
    "#;

    graphql(mutation, json!({ "fieldId": field_id, "options": merged }))?;

    if added.is_empty() {
        println!(
            "  {} Reordered status options into workflow order",
            "✓".green()
        );
    } else {
        println!(
            "  {} Added missing status options: {}",
            "✓".green(),
            added.join(", ")
        );
    }

    Ok(())
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
            "options": STATUS_OPTIONS
                .iter()
                .map(|(name, color, description)| json!({
                    "name": name,
                    "color": color,
                    "description": description,
                }))
                .collect::<Vec<_>>(),
        }),
    )?;

    let field_id = data["createProjectV2Field"]["projectV2Field"]["id"]
        .as_str()
        .context("Failed to create Status field")?
        .to_string();

    let names: Vec<&str> = STATUS_OPTIONS.iter().map(|(n, _, _)| *n).collect();
    println!(
        "  {} Created Status field ({})",
        "✓".green(),
        names.join(", ")
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

fn detect_owner_repo(yes: bool) -> Result<(String, String)> {
    if let Ok(out) = gh(&["repo", "view", "--json", "owner,name"]) {
        let json: serde_json::Value = serde_json::from_str(&out)?;
        let owner = json["owner"]["login"]
            .as_str()
            .context("No owner")?
            .to_string();
        let name = json["name"].as_str().context("No repo name")?.to_string();
        return Ok((owner, name));
    }

    if yes {
        anyhow::bail!(
            "Not in a GitHub repo, so --yes has nothing to detect. Pass --owner <owner> --repo <repo>."
        );
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

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(id: &str, name: &str) -> serde_json::Value {
        json!({ "id": id, "name": name, "color": "GRAY", "description": "" })
    }

    fn names(options: &[serde_json::Value]) -> Vec<String> {
        options
            .iter()
            .map(|o| o["name"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    #[test]
    fn canonical_list_is_in_workflow_order() {
        let order: Vec<&str> = STATUS_OPTIONS.iter().map(|(n, _, _)| *n).collect();
        assert_eq!(
            order,
            vec![
                "Backlog",
                "Todo",
                "In Progress",
                "Needs Decision",
                "In Review",
                "Done"
            ]
        );
    }

    #[test]
    fn a_github_default_board_lands_in_workflow_order() {
        let existing = vec![opt("a", "Todo"), opt("b", "In Progress"), opt("c", "Done")];
        let (merged, added) = merge_status_options(&existing);
        assert_eq!(added, vec!["Backlog", "Needs Decision", "In Review"]);
        assert_eq!(
            names(&merged),
            vec![
                "Backlog",
                "Todo",
                "In Progress",
                "Needs Decision",
                "In Review",
                "Done"
            ]
        );
    }

    #[test]
    fn a_complete_board_in_the_wrong_order_is_reordered() {
        let existing = vec![
            opt("a", "Todo"),
            opt("b", "In Progress"),
            opt("c", "Done"),
            opt("d", "Backlog"),
            opt("e", "Needs Decision"),
            opt("f", "In Review"),
        ];
        let (merged, added) = merge_status_options(&existing);
        assert!(added.is_empty());
        assert_eq!(
            names(&merged),
            vec![
                "Backlog",
                "Todo",
                "In Progress",
                "Needs Decision",
                "In Review",
                "Done"
            ]
        );
    }

    #[test]
    fn reordering_alone_still_counts_as_work_to_do() {
        let existing = vec![
            opt("a", "Todo"),
            opt("b", "In Progress"),
            opt("c", "Done"),
            opt("d", "Backlog"),
            opt("e", "Needs Decision"),
            opt("f", "In Review"),
        ];
        let (merged, _) = merge_status_options(&existing);
        assert!(!options_match(&existing, &merged));
    }

    #[test]
    fn a_custom_status_stays_after_the_option_it_followed() {
        let existing = vec![
            opt("a", "Todo"),
            opt("b", "In Progress"),
            opt("c", "Blocked On Legal"),
            opt("d", "Done"),
        ];
        let (merged, _) = merge_status_options(&existing);
        let merged_names = names(&merged);
        let after = merged_names
            .iter()
            .position(|n| n == "In Progress")
            .unwrap();
        assert_eq!(merged_names[after + 1], "Blocked On Legal");
    }

    #[test]
    fn a_custom_status_before_every_canonical_one_stays_at_the_front() {
        let existing = vec![opt("a", "Triage"), opt("b", "Todo")];
        let (merged, _) = merge_status_options(&existing);
        assert_eq!(names(&merged)[0], "Triage");
    }

    #[test]
    fn existing_option_ids_survive_a_merge() {
        let existing = vec![opt("keep-me", "Todo"), opt("keep-me-too", "Done")];
        let (merged, _) = merge_status_options(&existing);
        let find = |name: &str| {
            merged
                .iter()
                .find(|o| o["name"] == name)
                .unwrap()
                .get("id")
                .cloned()
                .unwrap()
        };
        assert_eq!(find("Todo"), "keep-me");
        assert_eq!(find("Done"), "keep-me-too");
    }

    #[test]
    fn newly_added_options_carry_no_id() {
        let existing = vec![opt("a", "Todo")];
        let (merged, _) = merge_status_options(&existing);
        let added = merged.iter().find(|o| o["name"] == "In Review").unwrap();
        assert!(added.get("id").is_none());
    }

    #[test]
    fn merging_a_complete_board_changes_nothing() {
        let existing: Vec<serde_json::Value> = STATUS_OPTIONS
            .iter()
            .enumerate()
            .map(|(i, (name, _, _))| opt(&format!("id{i}"), name))
            .collect();
        let (merged, added) = merge_status_options(&existing);
        assert!(added.is_empty());
        assert_eq!(names(&merged), names(&existing));
    }

    #[test]
    fn custom_statuses_survive_a_merge() {
        let existing = vec![opt("a", "Todo"), opt("b", "Blocked On Legal")];
        let (merged, _) = merge_status_options(&existing);
        assert!(names(&merged).contains(&"Blocked On Legal".to_string()));
    }

    #[test]
    fn a_board_already_in_canonical_order_needs_no_mutation() {
        let existing: Vec<serde_json::Value> = STATUS_OPTIONS
            .iter()
            .enumerate()
            .map(|(i, (name, _, _))| opt(&format!("id{i}"), name))
            .collect();
        let (merged, _) = merge_status_options(&existing);
        assert!(options_match(&existing, &merged));
    }

    #[test]
    fn a_lone_project_is_reused_without_asking() {
        assert_eq!(unattended_project_choice(&[7]).unwrap(), Some(7));
    }

    #[test]
    fn no_projects_means_create_one() {
        assert_eq!(unattended_project_choice(&[]).unwrap(), None);
    }

    #[test]
    fn several_projects_is_the_one_case_yes_refuses_to_guess() {
        let error = unattended_project_choice(&[7, 9]).unwrap_err().to_string();
        assert!(
            error.contains("--project"),
            "should name the way out: {error}"
        );
        assert!(error.contains("#7"), "should list the candidates: {error}");
    }

    #[test]
    fn option_names_match_case_insensitively() {
        let existing = vec![opt("a", "TODO"), opt("b", "needs decision")];
        let (_, added) = merge_status_options(&existing);
        assert!(!added.contains(&"Todo".to_string()));
        assert!(!added.contains(&"Needs Decision".to_string()));
    }
}
