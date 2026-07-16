//! Discovery of jig directories and the jigs within them.
//!
//! Project jigs live in `.gabarit/jigs/` directories, searched from the current
//! directory upward through its ancestors (nearest first). Global jigs live in
//! `~/.gabarit/jigs/`. On a name collision, the nearer source wins (project
//! shadows global).

use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::jig::{Jig, Source};

const JIGS_SUBDIR: &str = ".gabarit/jigs";
/// Reserved sibling of `jigs/` for test fixtures; never scanned for jigs.
const RESERVED_DIRS: &[&str] = &["fixtures"];

/// Ordered list of (jigs_dir, source), nearest project dir first, global last.
pub fn jig_dirs() -> Vec<(PathBuf, Source)> {
    let mut dirs = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        for ancestor in cwd.ancestors() {
            let d = ancestor.join(JIGS_SUBDIR);
            if d.is_dir() {
                dirs.push((d, Source::Project));
            }
        }
    }
    if let Some(home) = home_dir() {
        let d = home.join(JIGS_SUBDIR);
        if d.is_dir() {
            dirs.push((d, Source::Global));
        }
    }
    dirs
}

/// The directory new project jigs should be written to: the nearest existing
/// `.gabarit/jigs`, or `<cwd>/.gabarit/jigs` if none exists yet.
pub fn project_jigs_dir() -> PathBuf {
    if let Ok(cwd) = env::current_dir() {
        for ancestor in cwd.ancestors() {
            let d = ancestor.join(JIGS_SUBDIR);
            if d.is_dir() {
                return d;
            }
        }
        return cwd.join(JIGS_SUBDIR);
    }
    PathBuf::from(JIGS_SUBDIR)
}

/// Discover all jigs, deduplicated by name (first/nearest wins).
pub fn discover() -> Vec<Jig> {
    let mut jigs = Vec::new();
    let mut seen = HashSet::new();
    for (dir, source) in jig_dirs() {
        let mut files = Vec::new();
        collect_files(&dir, &dir, &mut files);
        for path in files {
            let name = name_from_path(&dir, &path);
            if !seen.insert(name.clone()) {
                continue; // shadowed by a nearer source
            }
            match Jig::load(&path, name, source) {
                Ok(j) => jigs.push(j),
                Err(e) => eprintln!("gabarit: skipping {}: {e}", path.display()),
            }
        }
    }
    jigs.sort_by(|a, b| a.name.cmp(&b.name));
    jigs
}

/// Find a single jig by name.
pub fn find(name: &str) -> Option<Jig> {
    discover().into_iter().find(|j| j.name == name)
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if dir == root && RESERVED_DIRS.contains(&name.as_ref()) {
                continue;
            }
            collect_files(root, &path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

/// Map a file path to a colon-separated jig name relative to its jigs dir.
/// `ci/logs.sh` -> `ci:logs`; a trailing `_default` component collapses to its
/// parent (`build/_default.sh` -> `build`).
pub fn name_from_path(root: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let mut components: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    if let Some(last) = components.last_mut() {
        // strip extension from the final component
        if let Some(dot) = last.rfind('.') {
            last.truncate(dot);
        }
    }
    if components.len() > 1 && components.last().map(|s| s == "_default").unwrap_or(false) {
        components.pop();
    }
    components
        .into_iter()
        .map(|c| c.replace(':', "_"))
        .collect::<Vec<_>>()
        .join(":")
}

fn home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .or_else(|| env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_mapping() {
        let root = Path::new("/p/.gabarit/jigs");
        assert_eq!(
            name_from_path(root, Path::new("/p/.gabarit/jigs/extract.sh")),
            "extract"
        );
        assert_eq!(
            name_from_path(root, Path::new("/p/.gabarit/jigs/ci/logs.sh")),
            "ci:logs"
        );
        assert_eq!(
            name_from_path(root, Path::new("/p/.gabarit/jigs/build/_default.sh")),
            "build"
        );
        assert_eq!(
            name_from_path(root, Path::new("/p/.gabarit/jigs/noext")),
            "noext"
        );
    }
}
