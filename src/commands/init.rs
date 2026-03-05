use anyhow::{Context, Result};
use colored::Colorize;
use serde_json::json;

use crate::config::{write_config, Config};
use crate::gh::{gh, graphql};

pub fn run(owner: Option<String>, repo: Option<String>, project_number: Option<u64>) -> Result<()> {
    // Detect owner/repo from gh if not provided
    let (owner, repo) = match (owner, repo) {
        (Some(o), Some(r)) => (o, r),
        _ => detect_owner_repo()?,
    };

    let project_number = match project_number {
        Some(n) => n,
        None => prompt_project_number(&owner, &repo)?,
    };

    println!("Fetching project fields for {owner}/{repo} project #{project_number}...");

    let query = r#"
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                projectV2(number: $number) {
                    id
                    fields(first: 20) {
                        nodes {
                            ... on ProjectV2SingleSelectField {
                                id
                                name
                                options { id name }
                            }
                        }
                    }
                }
            }
        }
    "#;

    let data = graphql(query, json!({ "owner": owner, "repo": repo, "number": project_number }))?;

    let project = &data["repository"]["projectV2"];
    let fields = project["fields"]["nodes"]
        .as_array()
        .context("No fields found on project")?;

    let status_field = fields
        .iter()
        .find(|f| f["name"].as_str().map(|n| n.eq_ignore_ascii_case("status")).unwrap_or(false))
        .with_context(|| "No 'Status' single-select field found on project. Create one first.")?;

    let priority_field = fields
        .iter()
        .find(|f| f["name"].as_str().map(|n| n.eq_ignore_ascii_case("priority")).unwrap_or(false))
        .with_context(|| "No 'Priority' single-select field found on project. Create one first.")?;

    let status_field_id = status_field["id"].as_str().context("Status field has no id")?.to_string();
    let priority_field_id = priority_field["id"].as_str().context("Priority field has no id")?.to_string();

    println!("  {} Status field: {}", "✓".green(), status_field_id);
    println!("  {} Priority field: {}", "✓".green(), priority_field_id);

    // Show available options for validation
    if let Some(opts) = status_field["options"].as_array() {
        let names: Vec<&str> = opts.iter().filter_map(|o| o["name"].as_str()).collect();
        println!("    Status options: {}", names.join(", "));
    }
    if let Some(opts) = priority_field["options"].as_array() {
        let names: Vec<&str> = opts.iter().filter_map(|o| o["name"].as_str()).collect();
        println!("    Priority options: {}", names.join(", "));
    }

    let config = Config {
        owner: owner.clone(),
        repo: repo.clone(),
        project_number,
        status_field_id,
        priority_field_id,
    };

    let cwd = std::env::current_dir()?;
    let config_path = cwd.join(".ghlobes.toml");
    write_config(&config, &config_path)?;

    println!("{} Wrote {}", "✓".green(), config_path.display());

    // Append agent instructions to CLAUDE.md
    let claude_md_path = cwd.join("CLAUDE.md");
    append_agent_instructions(&claude_md_path)?;
    println!("{} Updated {}", "✓".green(), claude_md_path.display());

    println!("{} ghlobes initialized for {owner}/{repo}", "✓".green());

    Ok(())
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
2. **Claim work:** Run `glb update <number> --claim` to mark it as in_progress.
3. **Do the work:** Implement the issue.
4. **Close:** Run `glb close <number>` when done. Include `--comment` with a brief summary.

### Commands

| Command | What it does |
|---|---|
| `glb ready` | Show issues ready to work (unblocked, not in progress) |
| `glb list` | List all open issues. Filters: `--status`, `--priority`, `--assignee` |
| `glb show <num>` | Show issue details, deps, status, priority |
| `glb create --title "..." --priority P1 --status open` | Create an issue |
| `glb update <num> --claim` | Claim issue (sets status to in_progress) |
| `glb update <num> --status <s> --priority <p>` | Update fields |
| `glb close <num>` | Close an issue |
| `glb reopen <num>` | Reopen a closed issue |
| `glb dep add <issue> <blocked_by>` | Add a dependency |
| `glb dep list <issue>` | Show dependencies |
| `glb blocked` | Show all blocked issues |
| `glb search "query"` | Search issues by text |
| `glb stats` | Show open/closed/blocked/ready counts |

### Rules

- **Always run `glb ready` at the start of a session** to find available work.
- **Always `--claim` before starting work** so other agents don't pick the same issue.
- **Never work on issues with status `in_progress`** — another agent is on it.
- **Create issues for new work** instead of just doing it. This keeps the project organized.
- **Add dependencies** when an issue can't be done until another is finished.
- **Close issues when done.** Don't leave them open.
"#
    );

    if path.exists() {
        let existing = std::fs::read_to_string(path)?;
        if existing.contains(marker) {
            // Already has instructions, replace them
            let before = existing.split(marker).next().unwrap_or("");
            std::fs::write(path, format!("{}{}", before.trim_end(), instructions))?;
        } else {
            // Append to existing
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
    let owner = json["owner"]["login"].as_str().context("No owner")?.to_string();
    let name = json["name"].as_str().context("No repo name")?.to_string();
    Ok((owner, name))
}

fn prompt_project_number(owner: &str, repo: &str) -> Result<u64> {
    // List projects so the user can pick one
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
        .context("No projects found")?;

    if projects.is_empty() {
        anyhow::bail!("No GitHub Projects found on {owner}/{repo}. Create one first.");
    }

    println!("Projects on {owner}/{repo}:");
    for p in projects {
        println!("  #{} — {}", p["number"], p["title"].as_str().unwrap_or("?"));
    }

    if projects.len() == 1 {
        let n = projects[0]["number"].as_u64().context("Bad project number")?;
        println!("Using project #{n}");
        return Ok(n);
    }

    anyhow::bail!("Multiple projects found. Pass --project <number> to specify which one.")
}
