use anyhow::{Context, Result};
use serde_json::Value;
use std::process::Command;

/// Run a GraphQL query via `gh api graphql` and return the parsed JSON.
pub fn graphql(query: &str, variables: Value) -> Result<Value> {
    let vars = variables.to_string();
    let output = Command::new("gh")
        .args(["api", "graphql", "-f", &format!("query={query}"), "-f", &format!("variables={vars}")])
        .output()
        .context("Failed to run `gh api graphql`. Is gh installed and authenticated?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh api graphql failed: {stderr}");
    }

    let json: Value = serde_json::from_slice(&output.stdout).context("Failed to parse gh api graphql output")?;

    if let Some(errors) = json.get("errors") {
        anyhow::bail!("GraphQL errors: {errors}");
    }

    Ok(json["data"].clone())
}

/// Run a raw gh CLI command, returning stdout as a string.
pub fn gh(args: &[&str]) -> Result<String> {
    let output = Command::new("gh")
        .args(args)
        .output()
        .with_context(|| format!("Failed to run `gh {}`", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("`gh {}` failed: {stderr}", args.join(" "));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Run gh and parse stdout as JSON.
pub fn gh_json(args: &[&str]) -> Result<Value> {
    let out = gh(args)?;
    let json: Value = serde_json::from_str(&out).context("Failed to parse gh output as JSON")?;
    Ok(json)
}
