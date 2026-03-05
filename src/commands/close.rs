use anyhow::Result;
use colored::Colorize;

use crate::config::find_config;
use crate::gh::gh;

pub fn run(number: u64, comment: Option<String>) -> Result<()> {
    let (config, _) = find_config()?;

    let repo = format!("{}/{}", config.owner, config.repo);
    let num_str = number.to_string();

    let mut args = vec!["issue", "close", &num_str, "--repo", &repo];

    let comment_str;
    if let Some(ref c) = comment {
        comment_str = c.clone();
        args.extend(["--comment", &comment_str]);
    }

    gh(&args)?;
    println!("{} Closed #{number}", "✓".green());

    Ok(())
}
