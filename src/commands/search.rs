use anyhow::Result;
use colored::Colorize;

use crate::config::find_config;
use crate::gh::gh_json;

pub fn run(query: &str) -> Result<()> {
    let (config, _) = find_config()?;

    let repo = format!("{}/{}", config.owner, config.repo);
    let results = gh_json(&[
        "issue", "list",
        "--repo", &repo,
        "--search", query,
        "--json", "number,title,state,labels",
        "--limit", "30",
    ])?;

    let empty = vec![];
    let issues = results.as_array().unwrap_or(&empty);

    if issues.is_empty() {
        println!("{}", "No issues found.".dimmed());
        return Ok(());
    }

    println!("{} results for \"{}\":", issues.len().to_string().green().bold(), query);
    println!("{}", "─".repeat(60).dimmed());

    for issue in issues {
        let number = issue["number"].as_u64().unwrap_or(0);
        let title = issue["title"].as_str().unwrap_or("?");
        let state = issue["state"].as_str().unwrap_or("?");
        let state_colored = match state {
            "OPEN" => state.green().to_string(),
            "CLOSED" => state.red().to_string(),
            _ => state.to_string(),
        };
        let trunc = if title.len() > 50 { format!("{}…", &title[..49]) } else { title.to_string() };
        println!("  #{:<6} {:<52} {}", number, trunc, state_colored);
    }

    Ok(())
}
