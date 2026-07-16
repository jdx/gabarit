//! `suggest`: mine coding-agent session transcripts for repeated shell command
//! shapes worth crystallizing into jigs.
//!
//! Claude Code stores per-project transcripts as JSONL under
//! `~/.claude/projects/<encoded-cwd>/*.jsonl`. Each Bash tool call is
//! normalized into a "shape" — program names and flags kept, operands replaced
//! with `_` — so `rg foo src | head -5` and `rg bar lib | head -20` count as
//! the same repeated operation.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use eyre::{eyre, Result};
use serde::Serialize;
use serde_json::Value;

/// Programs whose bare invocations are never worth a jig.
const TRIVIAL: &[&str] = &[
    "cd", "ls", "cat", "echo", "pwd", "which", "head", "tail", "mkdir", "rm", "cp", "mv", "touch",
    "true", "false", "sleep", "man", "less", "date", "env", "export", "set",
];

/// Programs whose first bare-word operand is a subcommand worth keeping in the
/// shape (`git log` and `git diff` are different operations; `rg foo` and
/// `rg bar` are not).
const SUBCOMMAND_PROGS: &[&str] = &[
    "git",
    "cargo",
    "npm",
    "pnpm",
    "yarn",
    "gh",
    "docker",
    "kubectl",
    "mise",
    "brew",
    "pip",
    "pip3",
    "uv",
    "go",
    "just",
    "bun",
    "deno",
    "rustup",
    "helm",
    "terraform",
    "aws",
    "gcloud",
];

#[derive(Debug, Serialize)]
pub struct Candidate {
    pub shape: String,
    pub occurrences: usize,
    pub sessions: usize,
    pub score: usize,
    pub suggested_name: String,
    pub examples: Vec<String>,
}

#[derive(Default)]
struct Bucket {
    occurrences: usize,
    sessions: std::collections::BTreeSet<String>,
    examples: Vec<String>,
}

pub struct Options {
    /// Transcript directory override (default: derived from cwd).
    pub dir: Option<PathBuf>,
    /// Minimum occurrences for a shape to be reported.
    pub min: usize,
    /// Maximum number of candidates to report.
    pub limit: usize,
}

pub fn suggest(opts: &Options) -> Result<Vec<Candidate>> {
    let dir = match &opts.dir {
        Some(d) => d.clone(),
        None => default_transcript_dir()?,
    };
    if !dir.is_dir() {
        return Err(eyre!(
            "no transcripts found at {} — has a coding agent run in this project?",
            dir.display()
        ));
    }

    let mut buckets: BTreeMap<String, Bucket> = BTreeMap::new();
    for entry in fs::read_dir(&dir)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        collect_from_transcript(&path, &mut buckets);
    }

    let mut candidates: Vec<Candidate> = buckets
        .into_iter()
        .filter(|(_, b)| b.occurrences >= opts.min)
        .map(|(shape, b)| {
            let sessions = b.sessions.len();
            Candidate {
                suggested_name: suggest_name(&shape),
                score: sessions * 2 + b.occurrences,
                occurrences: b.occurrences,
                sessions,
                examples: b.examples,
                shape,
            }
        })
        .collect();
    candidates.sort_by(|a, b| b.score.cmp(&a.score).then(a.shape.cmp(&b.shape)));
    candidates.truncate(opts.limit);
    Ok(candidates)
}

pub fn render(opts: &Options) -> Result<String> {
    let candidates = suggest(opts)?;
    if candidates.is_empty() {
        return Ok("no repeated command shapes found — nothing worth crystallizing yet\n".into());
    }
    let mut out = String::new();
    out.push_str(&format!(
        "{} repeated command shape(s) worth considering as jigs:\n\n",
        candidates.len()
    ));
    for c in &candidates {
        out.push_str(&format!(
            "{}× across {} session(s): {}\n",
            c.occurrences, c.sessions, c.shape
        ));
        for ex in c.examples.iter().take(2) {
            out.push_str(&format!("    e.g. {ex}\n"));
        }
        out.push_str(&format!(
            "    forge it: gabarit new {} --description \"...\"\n\n",
            c.suggested_name
        ));
    }
    Ok(out)
}

fn collect_from_transcript(path: &Path, buckets: &mut BTreeMap<String, Bucket>) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    for line in content.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }
        let session = v
            .get("sessionId")
            .and_then(|s| s.as_str())
            .unwrap_or("unknown")
            .to_string();
        let Some(content) = v.pointer("/message/content").and_then(|c| c.as_array()) else {
            continue;
        };
        for block in content {
            if block.get("type").and_then(|t| t.as_str()) != Some("tool_use")
                || block.get("name").and_then(|n| n.as_str()) != Some("Bash")
            {
                continue;
            }
            let Some(cmd) = block.pointer("/input/command").and_then(|c| c.as_str()) else {
                continue;
            };
            let Some(shape) = normalize(cmd) else {
                continue;
            };
            let bucket = buckets.entry(shape).or_default();
            bucket.occurrences += 1;
            bucket.sessions.insert(session.clone());
            let example = compact_example(cmd);
            if bucket.examples.len() < 3 && !bucket.examples.contains(&example) {
                bucket.examples.push(example);
            }
        }
    }
}

/// Normalize a command into a shape, or `None` if it isn't jig material.
pub fn normalize(cmd: &str) -> Option<String> {
    // Multi-line commands (heredocs, inline scripts) don't shape-match well.
    let cmd = cmd.trim();
    if cmd.len() > 500 || cmd.lines().count() > 1 {
        return None;
    }
    let tokens = shell_words::split(cmd).ok()?;
    if tokens.is_empty() {
        return None;
    }

    let mut shape: Vec<String> = Vec::new();
    let mut segment_starts = true; // next token is a program name
    let mut expect_subcommand = false; // next bare word is a subcommand
    let mut programs: Vec<String> = Vec::new();
    let mut flags = 0usize;

    for tok in &tokens {
        match tok.as_str() {
            "|" | "||" | "&&" | ";" => {
                shape.push(tok.clone());
                segment_starts = true;
                expect_subcommand = false;
            }
            t if segment_starts => {
                // env-var prefixes (FOO=bar cmd) don't end the program position
                if t.contains('=') && !t.starts_with('-') {
                    shape.push("_=_".into());
                } else {
                    shape.push(t.to_string());
                    programs.push(t.to_string());
                    segment_starts = false;
                    expect_subcommand = SUBCOMMAND_PROGS.contains(&base_name(t));
                }
            }
            t if t.starts_with('-') => {
                flags += 1;
                if t[1..].chars().all(|c| c.is_ascii_digit()) && t.len() > 1 {
                    // numeric flags like `head -5` carry an operand-like value
                    shape.push("-_".into());
                } else {
                    match t.split_once('=') {
                        Some((flag, _)) => shape.push(format!("{flag}=_")),
                        None => shape.push(t.to_string()),
                    }
                }
            }
            t if expect_subcommand && t.chars().all(|c| c.is_ascii_lowercase() || c == '-') => {
                shape.push(t.to_string());
                expect_subcommand = false;
            }
            _ => {
                expect_subcommand = false;
                if shape.last().map(|s| s == "_").unwrap_or(false) {
                    continue; // collapse consecutive operands
                }
                shape.push("_".into());
            }
        }
    }

    // Never suggest gabarit itself.
    if programs.iter().any(|p| p.contains("gabarit")) {
        return None;
    }
    // Complexity gate: a pipeline/chain, or one command with enough surface.
    let complex = programs.len() >= 2 || (tokens.len() >= 4 && flags >= 1);
    if !complex {
        return None;
    }
    // A single trivial program with no chain is never jig material.
    if programs.len() == 1 && TRIVIAL.contains(&base_name(&programs[0])) {
        return None;
    }

    Some(shape.join(" "))
}

fn base_name(prog: &str) -> &str {
    prog.rsplit('/').next().unwrap_or(prog)
}

/// Derive a kebab-case jig name from the programs in the shape.
fn suggest_name(shape: &str) -> String {
    let mut programs: Vec<&str> = Vec::new();
    let mut segment_starts = true;
    for tok in shape.split(' ') {
        match tok {
            "|" | "||" | "&&" | ";" => segment_starts = true,
            "_=_" => {}
            t if segment_starts => {
                let base = base_name(t);
                if !programs.contains(&base) {
                    programs.push(base);
                }
                segment_starts = false;
            }
            _ => {}
        }
    }
    let name = programs
        .into_iter()
        .take(3)
        .collect::<Vec<_>>()
        .join("-")
        .replace([':', '.'], "-");
    if name.is_empty() {
        "new-jig".into()
    } else {
        name
    }
}

fn compact_example(cmd: &str) -> String {
    let mut s = cmd.trim().replace('\n', " ");
    if s.len() > 120 {
        s.truncate(117);
        s.push_str("...");
    }
    s
}

/// `/Users/x/src/proj` -> `~/.claude/projects/-Users-x-src-proj`
fn default_transcript_dir() -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let encoded: String = cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| eyre!("cannot determine home directory"))?;
    Ok(PathBuf::from(home).join(".claude/projects").join(encoded))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_shape_different_operands() {
        let a = normalize("rg 'foo' src/ | head -5").unwrap();
        let b = normalize("rg 'bar baz' lib/ | head -20").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, "rg _ | head -_");
    }

    #[test]
    fn trivial_commands_rejected() {
        assert!(normalize("ls -la").is_none());
        assert!(normalize("cat foo.txt").is_none());
        assert!(normalize("cargo build").is_none()); // too few tokens, no flags
        assert!(normalize("gabarit ls --json | head -2").is_none());
    }

    #[test]
    fn multiline_rejected() {
        assert!(normalize("python3 <<EOF\nprint(1)\nEOF").is_none());
    }

    #[test]
    fn flag_values_normalized() {
        let a = normalize("git log --since=2024-01-01 --numstat -n 30").unwrap();
        let b = normalize("git log --since=yesterday --numstat -n 10").unwrap();
        assert_eq!(a, b);
        assert_eq!(a, "git log --since=_ --numstat -n _");
    }

    #[test]
    fn name_from_pipeline() {
        assert_eq!(suggest_name("rg _ | sort | uniq -c"), "rg-sort-uniq");
        assert_eq!(suggest_name("git log --numstat"), "git");
    }
}
