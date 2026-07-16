//! Parser for `#GABARIT key = value` comment headers.
//!
//! Mirrors the shape of mise's file-task metadata: each header line carries one
//! TOML `key = value` pair, and the block is terminated by the first line that
//! is not a shebang, a comment, or blank.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;

static HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:#|//|::)(?:GABARIT| ?\[GABARIT\])\s+(.+?)\s*$").unwrap());
static COMMENT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:#|//|::)").unwrap());

/// Recognized `#GABARIT` metadata keys. Unknown keys are collected in
/// [`Header::unknown`] so callers can warn without failing (forward-compat).
#[derive(Debug, Default, Clone, Serialize)]
pub struct Header {
    pub description: Option<String>,
    pub test: Option<String>,
    pub created_by: Option<String>,
    pub created_at: Option<String>,
    pub hide: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unknown: Vec<String>,
}

/// Parse the leading comment block of a script into a [`Header`].
pub fn parse(content: &str) -> Header {
    let mut header = Header::default();
    let mut table = toml::Table::new();

    for (i, line) in content.lines().enumerate() {
        // The very first line may be a shebang.
        if i == 0 && line.starts_with("#!") {
            continue;
        }
        if let Some(caps) = HEADER_RE.captures(line) {
            match caps[1].parse::<toml::Table>() {
                Ok(t) => {
                    for (k, v) in t {
                        table.insert(k, v);
                    }
                }
                Err(_) => header.unknown.push(caps[1].to_string()),
            }
            continue;
        }
        // Blank lines and non-GABARIT comments continue the block.
        if line.trim().is_empty() || COMMENT_RE.is_match(line) {
            continue;
        }
        // First real line of code ends the header block.
        break;
    }

    for (k, v) in table {
        match k.as_str() {
            "description" => header.description = v.as_str().map(String::from),
            "test" => header.test = v.as_str().map(String::from),
            "created-by" | "created_by" => header.created_by = v.as_str().map(String::from),
            "created-at" | "created_at" => {
                header.created_at = v.as_str().map(String::from).or_else(|| Some(v.to_string()));
            }
            "hide" => header.hide = v.as_bool().unwrap_or(false),
            other => header.unknown.push(other.to_string()),
        }
    }

    header
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_header() {
        let src = r#"#!/usr/bin/env bash
#GABARIT description = "Extract failing tests"
#GABARIT created-by = "claude-code"
#GABARIT test = "../fixtures/sample.log --json"
#USAGE arg "<logfile>"
set -euo pipefail
#GABARIT hide = true
"#;
        let h = parse(src);
        assert_eq!(h.description.as_deref(), Some("Extract failing tests"));
        assert_eq!(h.created_by.as_deref(), Some("claude-code"));
        assert_eq!(h.test.as_deref(), Some("../fixtures/sample.log --json"));
        // hide is after a code line, so it must NOT be picked up.
        assert!(!h.hide);
    }

    #[test]
    fn collects_unknown_keys() {
        let src = "#GABARIT description = \"x\"\n#GABARIT bogus = 1\n";
        let h = parse(src);
        assert_eq!(h.unknown, vec!["bogus".to_string()]);
    }
}
