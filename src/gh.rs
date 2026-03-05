use anyhow::{Context, Result};
use serde_json::Value;
use std::process::Command;

/// Run a GraphQL query via `gh api graphql` and return the parsed JSON.
///
/// Passes the full request body via stdin to handle complex variable types
/// (arrays, nested objects) that `gh api graphql -f/-F` can't express.
pub fn graphql(query: &str, variables: Value) -> Result<Value> {
    let body = serde_json::json!({
        "query": query,
        "variables": variables,
    });

    let output = Command::new("gh")
        .args(["api", "graphql", "--input", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to run `gh api graphql`. Is gh installed and authenticated?")
        .and_then(|mut child| {
            if let Some(ref mut stdin) = child.stdin {
                serde_json::to_writer(stdin, &body).context("Failed to write to gh stdin")?;
            }
            child.wait_with_output().context("Failed to wait for gh")
        })?;

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
