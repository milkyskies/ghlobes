use anyhow::Result;
use colored::Colorize;

use crate::config::find_config;
use crate::gh::gh;

pub fn run(number: u64) -> Result<()> {
    let (config, _) = find_config()?;

    let repo = format!("{}/{}", config.owner, config.repo);
    let num_str = number.to_string();

    gh(&["issue", "reopen", &num_str, "--repo", &repo])?;
    println!("{} Reopened #{number}", "✓".green());

    Ok(())
}
