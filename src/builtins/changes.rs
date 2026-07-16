//! `changes` built-in: a token-dense digest of recent git activity — commits,
//! the files with the most churn, and the current working-tree state. Shells out
//! to `git` rather than linking a git library.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;

use eyre::{eyre, Result};
use regex::Regex;

static DURATION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)([smhdw])$").unwrap());

pub fn render(dir: &Path, since: &str) -> Result<String> {
    if !is_git_repo(dir) {
        return Err(eyre!("{} is not inside a git repository", dir.display()));
    }

    let mut out = String::new();
    let (range_args, label) = since_to_git_args(since);

    // Commits.
    let mut log_args = vec![
        "log".to_string(),
        "--pretty=format:%h %ad %s".to_string(),
        "--date=short".to_string(),
        "-n".to_string(),
        "30".to_string(),
    ];
    log_args.extend(range_args.clone());
    let commits = git(dir, &log_args)?;
    let commit_lines: Vec<&str> = commits.lines().filter(|l| !l.is_empty()).collect();
    out.push_str(&format!("commits ({label}): {}\n", commit_lines.len()));
    for line in commit_lines.iter().take(15) {
        out.push_str(&format!("  {line}\n"));
    }
    if commit_lines.len() > 15 {
        out.push_str(&format!("  … {} more\n", commit_lines.len() - 15));
    }

    // Churn: sum of added+deleted lines per file.
    let mut churn_args = vec![
        "log".to_string(),
        "--numstat".to_string(),
        "--pretty=format:".to_string(),
    ];
    churn_args.extend(range_args);
    let numstat = git(dir, &churn_args)?;
    let mut churn: BTreeMap<String, i64> = BTreeMap::new();
    for line in numstat.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() == 3 {
            let add: i64 = cols[0].parse().unwrap_or(0);
            let del: i64 = cols[1].parse().unwrap_or(0);
            *churn.entry(cols[2].to_string()).or_insert(0) += add + del;
        }
    }
    if !churn.is_empty() {
        let mut ranked: Vec<(String, i64)> = churn.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out.push_str("hot files (by churn):\n");
        for (file, lines) in ranked.iter().take(15) {
            out.push_str(&format!("  {lines:>6}  {file}\n"));
        }
    }

    // Working tree.
    let status = git(dir, &["status".to_string(), "--porcelain".to_string()])?;
    let status_lines: Vec<&str> = status.lines().filter(|l| !l.is_empty()).collect();
    if status_lines.is_empty() {
        out.push_str("working tree: clean\n");
    } else {
        out.push_str(&format!("working tree: {} changed\n", status_lines.len()));
        for line in status_lines.iter().take(20) {
            out.push_str(&format!("  {line}\n"));
        }
        if status_lines.len() > 20 {
            out.push_str(&format!("  … {} more\n", status_lines.len() - 20));
        }
    }

    Ok(out)
}

/// Translate a `--since` value into git log arguments. A ref (or ref range)
/// becomes `<ref>..HEAD`; a duration like `7d` becomes `--since=...`.
fn since_to_git_args(since: &str) -> (Vec<String>, String) {
    if let Some(caps) = DURATION_RE.captures(since) {
        let n = &caps[1];
        let unit = match &caps[2] {
            "s" => "seconds",
            "m" => "minutes",
            "h" => "hours",
            "d" => "days",
            "w" => "weeks",
            _ => "days",
        };
        let expr = format!("{n} {unit} ago");
        (
            vec![format!("--since={expr}")],
            format!("last {n}{}", &caps[2]),
        )
    } else if since.contains("..") {
        (vec![since.to_string()], since.to_string())
    } else {
        (vec![format!("{since}..HEAD")], format!("since {since}"))
    }
}

fn is_git_repo(dir: &Path) -> bool {
    Command::new("git")
        .current_dir(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git(dir: &Path, args: &[String]) -> Result<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .map_err(|e| eyre!("running git: {e}"))?;
    if !out.status.success() {
        return Err(eyre!(
            "git {} failed: {}",
            args.first().cloned().unwrap_or_default(),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}
