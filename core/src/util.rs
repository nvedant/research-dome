//! Small helpers shared by the API handlers.

use chrono::{SecondsFormat, Utc};

/// Current time as an RFC 3339 UTC string with microsecond precision. Stored in
/// SQLite as TEXT and parsed natively by `new Date()` in the UI.
pub fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// Port of the original `safe_filename`: keep only the final path component and
/// replace anything outside `[A-Za-z0-9._-]` with `_`.
pub fn safe_filename(name: &str) -> String {
    let name = if name.is_empty() { "upload" } else { name };
    let last = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut out = String::with_capacity(last.len());
    let mut in_run = false;
    for ch in last.chars() {
        if ch.is_ascii_alphanumeric() || ch == '.' || ch == '_' || ch == '-' {
            out.push(ch);
            in_run = false;
        } else if !in_run {
            out.push('_');
            in_run = true;
        }
    }
    let trimmed = out.trim_matches(|c| c == '.' || c == '_');
    if trimmed.is_empty() {
        "upload".to_string()
    } else {
        trimmed.to_string()
    }
}

/// File stem (name without the final extension), like `Path(name).stem`.
pub fn file_stem(name: &str) -> String {
    match name.rfind('.') {
        Some(idx) if idx > 0 => name[..idx].to_string(),
        _ => name.to_string(),
    }
}

/// Lower-cased extension including the dot (`".pdf"`), or an empty string.
pub fn extension_lower(name: &str) -> String {
    match name.rfind('.') {
        Some(idx) if idx > 0 && idx + 1 < name.len() => name[idx..].to_lowercase(),
        _ => String::new(),
    }
}

/// Keep the last `max` characters of a string (the original `log[-20000:]`).
pub fn tail_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        text.to_string()
    } else {
        text.chars().skip(count - max).collect()
    }
}

/// Keep at most `max` characters (the original `value[:max]`).
pub fn head_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// True for `#rrggbb` hex colours.
pub fn is_hex_color(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 7 && bytes[0] == b'#' && bytes[1..].iter().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_filename_matches_original_behaviour() {
        assert_eq!(safe_filename("My Paper (v2).pdf"), "My_Paper_v2_.pdf");
        assert_eq!(safe_filename("../../etc/passwd"), "passwd");
        assert_eq!(safe_filename(""), "upload");
        assert_eq!(safe_filename("???"), "upload");
        assert_eq!(safe_filename("C:\\Users\\me\\paper one.pdf"), "paper_one.pdf");
    }

    #[test]
    fn stems_and_extensions() {
        assert_eq!(file_stem("paper.final.pdf"), "paper.final");
        assert_eq!(extension_lower("Figure.PNG"), ".png");
        assert_eq!(extension_lower("noext"), "");
    }

    #[test]
    fn tail_and_head() {
        assert_eq!(tail_chars("abcdef", 3), "def");
        assert_eq!(head_chars("abcdef", 3), "abc");
    }

    #[test]
    fn colours() {
        assert!(is_hex_color("#ffe66d"));
        assert!(!is_hex_color("ffe66d"));
        assert!(!is_hex_color("#ffe66"));
    }
}
