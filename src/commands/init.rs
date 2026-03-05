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
    update_claude_md: bool,
) -> Result<()> {
    if update_claude_md {
        let cwd = std::env::current_dir()?;
        let claude_md_path = cwd.join("CLAUDE.md");
        append_agent_instructions(&claude_md_path)?;
        println!("{} Updated {}", "✓".green(), claude_md_path.display());
        return Ok(());
    }

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
                                options { id name }
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

    let claude_md_path = cwd.join("CLAUDE.md");
    append_agent_instructions(&claude_md_path)?;
    println!("{} Updated {}", "✓".green(), claude_md_path.display());
    println!("  Tip: run `glb init --update-claude-md` anytime to refresh the agent instructions.");

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

    let title = prompt("Project title [ghlobes]: ")?;
    let title = if title.is_empty() {
        "ghlobes".to_string()
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
                { "name": "open", "color": "GREEN", "description": "" },
                { "name": "in_progress", "color": "YELLOW", "description": "" },
                { "name": "blocked", "color": "RED", "description": "" },
                { "name": "closed", "color": "GRAY", "description": "" },
            ],
        }),
    )?;

    let field_id = data["createProjectV2Field"]["projectV2Field"]["id"]
        .as_str()
        .context("Failed to create Status field")?
        .to_string();

    println!(
        "  {} Created Status field (open, in_progress, blocked, closed)",
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

fn append_agent_instructions(path: &std::path::Path) -> Result<()> {
    let marker = "<!-- glb-agent-instructions -->";
    let instructions = format!(
        r#"
{marker}
## Task Tracking with glb

This project uses `glb` (ghlobes) for issue tracking via GitHub Issues + Projects.
All state lives in GitHub — no local database.

### Workflow

1. **Find work:** Run `glb ready` to see unblocked, unclaimed issues.
2. **Claim work:** Run `glb update <number> --claim` to mark it as In Progress.
3. **Do the work:** Implement the issue.
4. **Close:** Run `glb close <number>` when done. Include `--comment` with a brief summary.

### Commands

| Command | What it does |
|---|---|
| `glb ready` | Show issues ready to work (unblocked, not in progress) |
| `glb list` | List all open issues. Filters: `--status`, `--priority`, `--assignee` |
| `glb show <num>` | Show issue details, deps, status, priority, points, sub-issues |
| `glb create --title "..." --priority P1 --status Todo --points 3` | Create an issue |
| `glb update <num> --claim` | Claim issue (sets status to In Progress) |
| `glb update <num> --status <s> --priority <p> --points <n>` | Update fields |
| `glb close <num>` | Close an issue |
| `glb reopen <num>` | Reopen a closed issue |
| `glb dep add <issue> <blocked_by>` | Add a blocking dependency |
| `glb dep list <issue>` | Show dependencies |
| `glb sub add <parent> <child>` | Add a sub-issue to a parent (epic) |
| `glb sub remove <parent> <child>` | Remove a sub-issue from a parent |
| `glb sub list <parent>` | List sub-issues with progress |
| `glb blocked` | Show all blocked issues |
| `glb search "query"` | Search issues by text |
| `glb stats` | Show open/closed/blocked/ready counts |
| `glb init --update-claude-md` | Refresh these agent instructions |

### Points

Use **Fibonacci numbers** for the `--points` field: `1, 2, 3, 5, 8, 13`.
This represents effort/complexity. When estimating, pick the closest Fibonacci value.
- `1` — trivial (< 1 hour)
- `2` — small (a few hours)
- `3` — medium (half a day)
- `5` — large (full day)
- `8` — very large (2–3 days)
- `13` — epic (break it down into sub-issues instead if possible)

### Epics (sub-issues)

Use `glb sub` to organize work into parent/child hierarchies (epics).
GitHub renders these natively with a progress bar on the parent issue.

```
# Create an epic and its tasks
glb create --title "Auth system"          # e.g. becomes #10
glb create --title "Design auth flow"     # e.g. becomes #11
glb create --title "Implement auth"       # e.g. becomes #12

# Link them
glb sub add 10 11
glb sub add 10 12

# Optional: make tasks sequential with a blocking dep
glb dep add 12 11   # #12 blocked by #11
```

### Rules

- **Always run `glb ready` at the start of a session** to find available work.
- **Always `--claim` before starting work** so other agents don't pick the same issue.
- **Never work on issues with status `In Progress`** — another agent is on it.
- **Create issues for new work** instead of just doing it. This keeps the project organized.
- **Add dependencies** when an issue can't be done until another is finished.
- **Close issues when done.** Don't leave them open.
"#
    );

    if path.exists() {
        let existing = std::fs::read_to_string(path)?;
        if existing.contains(marker) {
            let before = existing.split(marker).next().unwrap_or("");
            std::fs::write(path, format!("{}{}", before.trim_end(), instructions))?;
        } else {
            std::fs::write(path, format!("{}\n{}", existing.trim_end(), instructions))?;
        }
    } else {
        std::fs::write(path, format!("# CLAUDE.md\n{instructions}"))?;
    }

    Ok(())
}

fn detect_owner_repo() -> Result<(String, String)> {
    let out = gh(&["repo", "view", "--json", "owner,name"])?;
    let json: serde_json::Value = serde_json::from_str(&out)?;
    let owner = json["owner"]["login"]
        .as_str()
        .context("No owner")?
        .to_string();
    let name = json["name"].as_str().context("No repo name")?.to_string();
    Ok((owner, name))
}
