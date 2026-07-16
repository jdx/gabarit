//! `tree` built-in: a token-dense, gitignore-aware orientation map of a
//! directory. Optimized for information-per-token, not glanceability:
//! repetitive directories collapse to a summary line, elision is always
//! explicit, and a token budget drives how deep the map renders.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::SystemTime;

use eyre::Result;
use ignore::WalkBuilder;

const FILE_LIST_LIMIT: usize = 12;
const MAX_RENDER_DEPTH: usize = 20;

#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: Vec<FileInfo>,
}

struct FileInfo {
    name: String,
    ext: String,
    mtime: Option<SystemTime>,
}

struct Stats {
    files: usize,
    dirs: usize,
    ext_hist: BTreeMap<String, usize>,
    oldest: Option<SystemTime>,
    newest: Option<SystemTime>,
}

pub struct Options {
    pub budget: usize,
    pub max_depth: Option<usize>,
}

pub fn render(root: &Path, opts: &Options) -> Result<String> {
    let tree = build_tree(root)?;
    let hard_max = opts
        .max_depth
        .unwrap_or(MAX_RENDER_DEPTH)
        .min(MAX_RENDER_DEPTH);

    // Iterative deepening: render as deep as the budget allows.
    let mut best = render_at_depth(root, &tree, 1);
    for depth in 2..=hard_max {
        let candidate = render_at_depth(root, &tree, depth);
        if approx_tokens(&candidate) > opts.budget {
            break;
        }
        let grew = candidate.len() != best.len();
        best = candidate;
        if !grew {
            break; // fully expanded; deeper won't add anything
        }
    }
    Ok(best)
}

fn build_tree(root: &Path) -> Result<Node> {
    let mut tree = Node::default();
    let walk = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .parents(true)
        .build();
    for entry in walk.flatten() {
        if entry.depth() == 0 {
            continue;
        }
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let components: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .collect();
        if components.is_empty() {
            continue;
        }
        let mut node = &mut tree;
        for comp in &components[..components.len() - 1] {
            node = node.dirs.entry(comp.clone()).or_default();
        }
        let last = components.last().unwrap().clone();
        if is_dir {
            node.dirs.entry(last).or_default();
        } else {
            let ext = Path::new(&last)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_string();
            let mtime = entry.metadata().ok().and_then(|m| m.modified().ok());
            node.files.push(FileInfo {
                name: last,
                ext,
                mtime,
            });
        }
    }
    Ok(tree)
}

fn stats(node: &Node) -> Stats {
    let mut s = Stats {
        files: 0,
        dirs: 0,
        ext_hist: BTreeMap::new(),
        oldest: None,
        newest: None,
    };
    fn recurse(node: &Node, s: &mut Stats) {
        for f in &node.files {
            s.files += 1;
            *s.ext_hist.entry(ext_label(&f.ext)).or_insert(0) += 1;
            if let Some(t) = f.mtime {
                s.oldest = Some(s.oldest.map_or(t, |o| o.min(t)));
                s.newest = Some(s.newest.map_or(t, |n| n.max(t)));
            }
        }
        for child in node.dirs.values() {
            s.dirs += 1;
            recurse(child, s);
        }
    }
    recurse(node, &mut s);
    s
}

fn render_at_depth(root: &Path, tree: &Node, max_depth: usize) -> String {
    let mut out = String::new();
    let s = stats(tree);
    out.push_str(&format!(
        "{} — {} files · {} dirs{}\n",
        display_root(root),
        s.files,
        s.dirs,
        date_range_suffix(&s),
    ));
    render_node(tree, "", 1, max_depth, &mut out);
    out
}

fn render_node(node: &Node, prefix: &str, depth: usize, max_depth: usize, out: &mut String) {
    // Directories first.
    for (name, child) in &node.dirs {
        if depth >= max_depth {
            let s = stats(child);
            out.push_str(&format!("{prefix}{name}/ — {}\n", summarize(&s),));
        } else {
            out.push_str(&format!("{prefix}{name}/\n"));
            render_node(child, &format!("{prefix}  "), depth + 1, max_depth, out);
        }
    }

    // Files: list if few, else aggregate.
    if node.files.len() > FILE_LIST_LIMIT {
        let s = stats(&Node {
            dirs: BTreeMap::new(),
            files: node.files.iter().map(clone_file).collect(),
        });
        out.push_str(&format!(
            "{prefix}({} files: {}{})\n",
            node.files.len(),
            hist_label(&s.ext_hist),
            date_range_suffix(&s),
        ));
    } else {
        let mut names: Vec<&FileInfo> = node.files.iter().collect();
        names.sort_by(|a, b| a.name.cmp(&b.name));
        for f in names {
            out.push_str(&format!("{prefix}{}\n", f.name));
        }
    }
}

fn summarize(s: &Stats) -> String {
    if s.dirs == 0 {
        format!(
            "{} files ({}){}",
            s.files,
            hist_label(&s.ext_hist),
            date_range_suffix(s)
        )
    } else {
        format!(
            "{} files, {} dirs ({}){}",
            s.files,
            s.dirs,
            hist_label(&s.ext_hist),
            date_range_suffix(s)
        )
    }
}

fn hist_label(hist: &BTreeMap<String, usize>) -> String {
    let mut items: Vec<(&String, &usize)> = hist.iter().collect();
    items.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    items
        .iter()
        .take(4)
        .map(|(ext, n)| format!("{n} {ext}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn ext_label(ext: &str) -> String {
    if ext.is_empty() {
        "(none)".to_string()
    } else {
        format!(".{ext}")
    }
}

fn date_range_suffix(s: &Stats) -> String {
    match (s.oldest, s.newest) {
        (Some(o), Some(n)) => {
            let od = fmt_month(o);
            let nd = fmt_month(n);
            if od == nd {
                format!(" · {nd}")
            } else {
                format!(" · {od}..{nd}")
            }
        }
        _ => String::new(),
    }
}

fn fmt_month(t: SystemTime) -> String {
    let dt: chrono::DateTime<chrono::Local> = t.into();
    dt.format("%Y-%m").to_string()
}

fn display_root(root: &Path) -> String {
    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| root.to_string_lossy().to_string());
    format!("{name}/")
}

fn clone_file(f: &FileInfo) -> FileInfo {
    FileInfo {
        name: f.name.clone(),
        ext: f.ext.clone(),
        mtime: f.mtime,
    }
}

fn approx_tokens(s: &str) -> usize {
    s.len() / 4
}
