use std::collections::HashSet;

use anyhow::{Context, Result};
use colored::Colorize;

use crate::config::find_config;
use crate::gh::gh_json;
use crate::util::truncate;

pub fn run(since: Option<String>, in_epic: Option<u64>, limit: usize) -> Result<()> {
    let (config, _) = find_config()?;

    // Resolve `since` into an absolute date string in the format gh search expects (YYYY-MM-DD).
    // Accept relative shorthand: 1d, 7d, 2w, 1m
    let since_date = match since.as_deref() {
        Some(s) => Some(parse_since(s).context("Could not parse --since value")?),
        None => None,
    };

    // Build a gh search query
    let mut search_parts = vec!["is:issue".to_string(), "is:closed".to_string()];
    if let Some(ref d) = since_date {
        search_parts.push(format!("closed:>={d}"));
    }
    let search = search_parts.join(" ");

    let repo = format!("{}/{}", config.owner, config.repo);
    let limit_str = limit.to_string();
    let results = gh_json(&[
        "issue",
        "list",
        "--repo",
        &repo,
        "--state",
        "closed",
        "--search",
        &search,
        "--json",
        "number,title,closedAt,labels",
        "--limit",
        &limit_str,
    ])?;

    // Optional epic filter — fetch the epic's transitive sub-issue numbers
    let epic_filter: Option<HashSet<u64>> = match in_epic {
        Some(epic_num) => Some(fetch_epic_subs(&config.owner, &config.repo, epic_num)?),
        None => None,
    };

    let empty = vec![];
    let issues = results.as_array().unwrap_or(&empty);

    let mut rows: Vec<(u64, String, String)> = issues
        .iter()
        .filter_map(|i| {
            let number = i["number"].as_u64()?;
            if let Some(ref filter) = epic_filter {
                if !filter.contains(&number) {
                    return None;
                }
            }
            let title = i["title"].as_str().unwrap_or("?").to_string();
            let closed_at = i["closedAt"].as_str().unwrap_or("").to_string();
            Some((number, title, closed_at))
        })
        .collect();

    // Sort newest first
    rows.sort_by(|a, b| b.2.cmp(&a.2));

    if rows.is_empty() {
        println!("{}", "No closed issues match.".dimmed());
        return Ok(());
    }

    let header = match (since.as_deref(), in_epic) {
        (Some(s), Some(e)) => format!("Closed since {s} in epic #{e}"),
        (Some(s), None) => format!("Closed since {s}"),
        (None, Some(e)) => format!("Closed in epic #{e}"),
        (None, None) => "Recently closed".to_string(),
    };
    println!(
        "{}: {}",
        header.bold(),
        format!("{} issues", rows.len()).cyan()
    );
    println!("{}", "─".repeat(70).dimmed());

    for (num, title, closed_at) in rows {
        let date = closed_at.get(..10).unwrap_or(&closed_at).to_string();
        let title_trunc = truncate(&title, 52);
        println!("  {} #{num:<5} {title_trunc}", date.dimmed());
    }

    Ok(())
}

/// Parse a `--since` value into a YYYY-MM-DD date.
/// Accepts: "2025-04-09", "1d", "7d", "2w", "1m" (months as 30 days).
fn parse_since(s: &str) -> Result<String> {
    // Already an absolute date?
    if s.len() == 10 && s.as_bytes().get(4) == Some(&b'-') {
        return Ok(s.to_string());
    }

    // Relative: <number><unit>
    let unit = s.chars().last().context("empty --since")?;
    let num_str = &s[..s.len() - unit.len_utf8()];
    let n: i64 = num_str
        .parse()
        .with_context(|| format!("Bad number in --since: {num_str}"))?;
    let days = match unit {
        'd' => n,
        'w' => n * 7,
        'm' => n * 30,
        'y' => n * 365,
        _ => anyhow::bail!("Unknown --since unit: {unit} (use d/w/m/y or YYYY-MM-DD)"),
    };

    // Compute the date by shelling to `date` (avoids pulling in chrono).
    // Works on macOS and GNU coreutils.
    let output = std::process::Command::new("date")
        .args(["-u", "-v", &format!("-{days}d"), "+%Y-%m-%d"])
        .output();
    if let Ok(out) = output {
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
        }
    }
    // GNU date fallback
    let output = std::process::Command::new("date")
        .args(["-u", "-d", &format!("{days} days ago"), "+%Y-%m-%d"])
        .output()
        .context("Failed to run `date` to compute relative time")?;
    if !output.status.success() {
        anyhow::bail!("`date` command failed for --since");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Fetch the transitive set of sub-issue numbers under the given epic, including
/// closed ones (since this command shows closed issues).
fn fetch_epic_subs(owner: &str, repo: &str, root: u64) -> Result<HashSet<u64>> {
    use serde_json::json;

    let query = r#"
        query($owner: String!, $repo: String!, $number: Int!) {
            repository(owner: $owner, name: $repo) {
                issue(number: $number) {
                    subIssues(first: 100) {
                        nodes { number }
                    }
                }
            }
        }
    "#;

    let mut visited = HashSet::new();
    let mut to_fetch = vec![root];
    visited.insert(root);

    while let Some(num) = to_fetch.pop() {
        let data = crate::gh::graphql(
            query,
            json!({
                "owner": owner,
                "repo": repo,
                "number": num,
            }),
        )?;
        let subs = data["repository"]["issue"]["subIssues"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for sub in subs {
            if let Some(n) = sub["number"].as_u64() {
                if visited.insert(n) {
                    to_fetch.push(n);
                }
            }
        }
    }
    Ok(visited)
}
