//! Create new jig files — used by both `gabarit new` and the `gabarit_new` MCP
//! tool so agents can forge a jig with or without shell access.

use std::fs;
use std::path::PathBuf;

use eyre::{eyre, Result};

use crate::discovery;

pub struct NewJig {
    pub name: String,
    pub description: Option<String>,
    pub interpreter: String,
    /// Full file contents. When `None`, a template is generated.
    pub content: Option<String>,
}

pub struct Created {
    pub path: PathBuf,
    pub name: String,
}

/// Who is forging this jig — an agent (detected from the environment) or the user.
pub fn detect_author() -> String {
    if std::env::var_os("CLAUDECODE").is_some()
        || std::env::var_os("CLAUDE_CODE_ENTRYPOINT").is_some()
    {
        "claude-code".to_string()
    } else if let Ok(user) = std::env::var("USER") {
        user
    } else {
        "unknown".to_string()
    }
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

fn extension_for(interpreter: &str) -> &'static str {
    match interpreter {
        "python" | "python3" => "py",
        "node" | "deno" | "bun" => "js",
        "ruby" => "rb",
        "perl" => "pl",
        _ => "sh",
    }
}

fn shebang_for(interpreter: &str) -> String {
    let prog = match interpreter {
        "python" => "python3",
        other => other,
    };
    format!("#!/usr/bin/env {prog}")
}

pub fn create(spec: &NewJig) -> Result<Created> {
    if spec.name.is_empty() {
        return Err(eyre!("jig name is required"));
    }
    let root = discovery::project_jigs_dir();
    let rel: PathBuf = spec.name.split(':').collect();
    let ext = extension_for(&spec.interpreter);
    let path = root.join(rel).with_extension(ext);

    if path.exists() {
        return Err(eyre!("jig already exists at {}", path.display()));
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let content = match &spec.content {
        Some(c) => normalize_provided(c, &spec.interpreter),
        None => template(spec),
    };
    fs::write(&path, content)?;
    make_executable(&path);

    Ok(Created {
        path,
        name: spec.name.clone(),
    })
}

/// Ensure agent-provided content at least has a shebang so it is runnable.
fn normalize_provided(content: &str, interpreter: &str) -> String {
    if content.starts_with("#!") {
        content.to_string()
    } else {
        format!("{}\n{content}", shebang_for(interpreter))
    }
}

fn template(spec: &NewJig) -> String {
    let desc = spec
        .description
        .clone()
        .unwrap_or_else(|| spec.name.clone());
    let body = match extension_for(&spec.interpreter) {
        "py" => "import os\n\nprint(\"TODO: implement\")\n",
        "js" => "console.log(\"TODO: implement\");\n",
        "rb" => "puts \"TODO: implement\"\n",
        _ => "set -euo pipefail\n\necho \"TODO: implement\"\n",
    };
    format!(
        "{shebang}\n#GABARIT description = {desc:?}\n#GABARIT created-by = {author:?}\n#GABARIT created-at = {date}\n#USAGE flag \"--example\" help=\"replace with your real args\"\n\n{body}",
        shebang = shebang_for(&spec.interpreter),
        desc = desc,
        author = detect_author(),
        date = today(),
        body = body,
    )
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(perms.mode() | 0o755);
        let _ = fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn make_executable(_path: &std::path::Path) {}
