//! A jig: a single self-describing script that declares its interface via
//! `#USAGE` and `#GABARIT` comment headers.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use eyre::{Result, WrapErr};
use serde::Serialize;
use usage::Spec;

use crate::header::{self, Header};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Project,
    Global,
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Source::Project => write!(f, "project"),
            Source::Global => write!(f, "global"),
        }
    }
}

pub struct Jig {
    /// Colon-separated name derived from the path under the jigs dir.
    pub name: String,
    pub path: PathBuf,
    pub source: Source,
    pub header: Header,
    pub spec: Spec,
}

impl Jig {
    pub fn load(path: &Path, name: String, source: Source) -> Result<Self> {
        let content =
            fs::read_to_string(path).wrap_err_with(|| format!("reading jig {}", path.display()))?;
        let header = header::parse(&content);
        let mut spec = Spec::parse_script(path)
            .map_err(|e| eyre::eyre!("parsing usage spec in {}: {e}", path.display()))?;
        // Ensure a stable bin name for help rendering and arg parsing.
        if spec.bin.is_empty() {
            spec.bin = name.clone();
        }
        if spec.name.is_empty() {
            spec.name = name.clone();
        }
        Ok(Jig {
            name,
            path: path.to_path_buf(),
            source,
            header,
            spec,
        })
    }

    /// The one-line description shown in listings and as the MCP tool
    /// description. Falls back to the spec's about text.
    pub fn description(&self) -> Option<String> {
        self.header
            .description
            .clone()
            .or_else(|| self.spec.cmd.help.clone())
            .or_else(|| self.spec.about.clone())
    }

    /// The MCP tool name for this jig: `jig_<name>` with `:` replaced by `_`.
    pub fn tool_name(&self) -> String {
        format!("jig_{}", self.name.replace(':', "_"))
    }
}
