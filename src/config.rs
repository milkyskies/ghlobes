use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const CONFIG_FILE: &str = ".ghlobes.toml";

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    pub owner: String,
    pub repo: String,
    pub project_number: u64,
    pub status_field_id: String,
    pub priority_field_id: String,
}

pub fn find_config() -> Result<(Config, PathBuf)> {
    let mut dir = std::env::current_dir()?;
    loop {
        let candidate = dir.join(CONFIG_FILE);
        if candidate.exists() {
            let content = std::fs::read_to_string(&candidate)
                .with_context(|| format!("Failed to read {}", candidate.display()))?;
            let config: Config = toml::from_str(&content)
                .with_context(|| format!("Failed to parse {}", candidate.display()))?;
            return Ok((config, candidate));
        }
        if !dir.pop() {
            anyhow::bail!(
                "No .ghlobes.toml found. Run `glb init` to set up this repository."
            );
        }
    }
}

pub fn write_config(config: &Config, path: &PathBuf) -> Result<()> {
    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content)?;
    Ok(())
}
