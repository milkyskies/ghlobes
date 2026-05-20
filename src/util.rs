/// Truncate a string to at most `max` characters (not bytes), appending an
/// ellipsis if truncated. Safe with multi-byte UTF-8 (e.g. em dashes, emoji).
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{truncated}…")
}
