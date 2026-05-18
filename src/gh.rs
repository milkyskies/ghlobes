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
        anyhow::bail!("gh api graphql failed: {stderr}{}", scope_hint(&stderr));
    }

    let json: Value = serde_json::from_slice(&output.stdout).context("Failed to parse gh api graphql output")?;

    if let Some(errors) = json.get("errors") {
        let msg = errors.to_string();
        anyhow::bail!("GraphQL errors: {msg}{}", scope_hint(&msg));
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

/// If the error mentions a missing GitHub OAuth scope, return a hint with the
/// `gh auth refresh` command needed to add it. Returns an empty string otherwise.
fn scope_hint(msg: &str) -> String {
    let mut scopes: Vec<&str> = Vec::new();
    for scope in [
        "read:project",
        "project",
        "repo",
        "read:org",
        "write:org",
        "admin:org",
        "workflow",
        "gist",
        "user",
        "read:user",
    ] {
        let needle = format!("'{scope}'");
        if msg.contains(&needle) && !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    if scopes.is_empty() {
        return String::new();
    }
    format!(
        "\n\nHint: your gh token is missing required scope(s). Run:\n  gh auth refresh -s {}",
        scopes.join(",")
    )
}
