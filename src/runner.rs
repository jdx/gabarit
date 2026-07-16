//! Execute a jig: parse argv against its usage spec, inject `usage_<name>` env
//! vars (identical to mise's file-task convention), and run it via its shebang
//! interpreter (no exec bit required).

use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use eyre::{eyre, Result};

use crate::jig::Jig;

pub struct Outcome {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Parse argv against the jig's spec and return the `usage_*` env vars.
/// Returns `Err` with the rendered help text when parsing fails.
pub fn parse_to_env(
    jig: &Jig,
    args: &[String],
) -> std::result::Result<Vec<(String, String)>, String> {
    let mut input = Vec::with_capacity(args.len() + 1);
    input.push(jig.spec.bin.clone());
    input.extend(args.iter().cloned());

    let env: HashMap<String, String> = std::env::vars().collect();
    match usage::Parser::new(&jig.spec).with_env(env).parse(&input) {
        Ok(po) => Ok(po.as_env().into_iter().collect()),
        Err(e) => {
            let help = usage::docs::cli::render_help(&jig.spec, &jig.spec.cmd, false);
            Err(format!("{e}\n\n{help}"))
        }
    }
}

/// Build the command to execute a jig, honoring exec bit, then shebang, then
/// falling back to an interpreter chosen by file extension.
fn build_command(path: &Path) -> Command {
    if is_executable(path) {
        return Command::new(path);
    }
    if let Some((prog, args)) = interpreter_from_shebang(path) {
        let mut cmd = Command::new(prog);
        cmd.args(args);
        cmd.arg(path);
        return cmd;
    }
    let prog = match path.extension().and_then(|e| e.to_str()) {
        Some("sh" | "bash") => "bash",
        Some("py") => "python3",
        Some("js" | "mjs" | "cjs") => "node",
        Some("rb") => "ruby",
        Some("pl") => "perl",
        _ => "sh",
    };
    let mut cmd = Command::new(prog);
    cmd.arg(path);
    cmd
}

/// Parse `#!/usr/bin/env -S bash -e` style shebangs into (program, leading args).
fn interpreter_from_shebang(path: &Path) -> Option<(String, Vec<String>)> {
    let content = fs::read_to_string(path).ok()?;
    let first = content.lines().next()?;
    let rest = first.strip_prefix("#!")?.trim();
    let mut parts = shell_words::split(rest).ok()?;
    if parts.is_empty() {
        return None;
    }
    let mut prog = parts.remove(0);
    if prog == "env" || prog.ends_with("/env") {
        while parts.first().map(|p| p == "-S").unwrap_or(false) {
            parts.remove(0);
        }
        if parts.is_empty() {
            return None;
        }
        prog = parts.remove(0);
    }
    Some((prog, parts))
}

/// Run a jig with inherited stdio (CLI path); returns its exit code.
pub fn run_inherited(jig: &Jig, args: &[String]) -> Result<i32> {
    let env = parse_to_env(jig, args).map_err(|help| eyre!(help))?;
    let mut cmd = build_command(&jig.path);
    cmd.envs(env);
    cmd.args(args);
    let status = cmd
        .status()
        .map_err(|e| eyre!("failed to run {}: {e}", jig.path.display()))?;
    Ok(status.code().unwrap_or(1))
}

/// Run a jig capturing stdout/stderr (MCP path).
pub fn run_captured(jig: &Jig, args: &[String]) -> Result<Outcome> {
    let env = parse_to_env(jig, args).map_err(|help| eyre!(help))?;
    let mut cmd = build_command(&jig.path);
    cmd.envs(env);
    cmd.args(args);
    let out = cmd
        .output()
        .map_err(|e| eyre!("failed to run {}: {e}", jig.path.display()))?;
    Ok(Outcome {
        status: out.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    })
}

/// Run a jig's declared smoke test from its own directory. Returns the outcome,
/// or `Ok(None)` if the jig declares no test.
pub fn run_test(jig: &Jig) -> Result<Option<Outcome>> {
    let Some(test) = &jig.header.test else {
        return Ok(None);
    };
    let args = shell_words::split(test).map_err(|e| eyre!("invalid test args: {e}"))?;
    let env = parse_to_env(jig, &args).map_err(|help| eyre!(help))?;
    let mut cmd = build_command(&jig.path);
    cmd.envs(env);
    cmd.args(&args);
    if let Some(dir) = jig.path.parent() {
        cmd.current_dir(dir);
    }
    let out = cmd
        .output()
        .map_err(|e| eyre!("failed to run test for {}: {e}", jig.name))?;
    Ok(Some(Outcome {
        status: out.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    }))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(p)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_p: &Path) -> bool {
    false
}
