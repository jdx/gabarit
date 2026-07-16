//! Command-line interface. The CLI is the bash-native frontend; `gabarit mcp`
//! exposes the same jigs to agents over the Model Context Protocol.

use std::path::PathBuf;
use std::process::exit;

use clap::{Parser, Subcommand};
use eyre::Result;
use serde_json::json;

use crate::builtins::{changes, tree};
use crate::{discovery, mcp, runner, scaffold, schema};

#[derive(Parser)]
#[command(
    name = "gabarit",
    version,
    about = "Toolbelt for coding agents — forge, discover, and run project-local tools (jigs)"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List discovered jigs
    Ls {
        #[arg(long)]
        json: bool,
    },
    /// Scaffold a new jig
    New {
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long, default_value = "bash")]
        interpreter: String,
    },
    /// Run a jig by name
    Run {
        name: String,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Show a jig's metadata and help
    Info {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Run jigs' declared smoke tests (for CI)
    Test { names: Vec<String> },
    /// Print a token-dense map of a directory
    Tree {
        path: Option<PathBuf>,
        #[arg(long, default_value_t = 2000)]
        budget: usize,
        #[arg(long)]
        depth: Option<usize>,
    },
    /// Summarize recent git activity
    Changes {
        #[arg(long, default_value = "7d")]
        since: String,
    },
    /// Run the MCP server over stdio
    Mcp,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Ls { json } => cmd_ls(json),
        Commands::New {
            name,
            description,
            interpreter,
        } => cmd_new(name, description, interpreter),
        Commands::Run { name, args } => cmd_run(name, args),
        Commands::Info { name, json } => cmd_info(name, json),
        Commands::Test { names } => cmd_test(names),
        Commands::Tree {
            path,
            budget,
            depth,
        } => cmd_tree(path, budget, depth),
        Commands::Changes { since } => cmd_changes(since),
        Commands::Mcp => mcp::serve().await,
    }
}

fn cmd_ls(as_json: bool) -> Result<()> {
    let jigs = discovery::discover();
    if as_json {
        let rows: Vec<_> = jigs
            .iter()
            .map(|j| {
                json!({
                    "name": j.name,
                    "description": j.description(),
                    "source": j.source.to_string(),
                    "path": j.path,
                    "has_test": j.header.test.is_some(),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if jigs.is_empty() {
        println!("no jigs found. create one with: gabarit new <name>");
        return Ok(());
    }
    let width = jigs.iter().map(|j| j.name.len()).max().unwrap_or(0);
    for j in &jigs {
        let test_mark = if j.header.test.is_some() {
            " ✓test"
        } else {
            ""
        };
        println!(
            "{:width$}  {}{}",
            j.name,
            j.description().unwrap_or_default(),
            test_mark,
            width = width
        );
    }
    Ok(())
}

fn cmd_new(name: String, description: Option<String>, interpreter: String) -> Result<()> {
    let created = scaffold::create(&scaffold::NewJig {
        name,
        description,
        interpreter,
        content: None,
    })?;
    println!(
        "created jig '{}' at {}",
        created.name,
        created.path.display()
    );
    println!(
        "next: edit the file, then run `gabarit run {}`",
        created.name
    );
    Ok(())
}

fn cmd_run(name: String, args: Vec<String>) -> Result<()> {
    let Some(jig) = discovery::find(&name) else {
        eprintln!("no jig named '{name}'");
        exit(1);
    };
    match runner::run_inherited(&jig, &args) {
        Ok(code) => exit(code),
        Err(e) => {
            eprintln!("{e}");
            exit(1);
        }
    }
}

fn cmd_info(name: String, as_json: bool) -> Result<()> {
    let Some(jig) = discovery::find(&name) else {
        eprintln!("no jig named '{name}'");
        exit(1);
    };
    if as_json {
        let out = json!({
            "name": jig.name,
            "path": jig.path,
            "source": jig.source.to_string(),
            "header": jig.header,
            "spec": jig.spec,
            "input_schema": schema::to_input_schema(&jig.spec),
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    println!("{}  ({})", jig.name, jig.source);
    println!("path: {}", jig.path.display());
    if let Some(d) = jig.description() {
        println!("description: {d}");
    }
    if let Some(by) = &jig.header.created_by {
        println!("created-by: {by}");
    }
    println!();
    println!(
        "{}",
        usage::docs::cli::render_help(&jig.spec, &jig.spec.cmd, false)
    );
    Ok(())
}

fn cmd_test(names: Vec<String>) -> Result<()> {
    let jigs = discovery::discover();
    let selected: Vec<_> = jigs
        .into_iter()
        .filter(|j| names.is_empty() || names.contains(&j.name))
        .collect();

    let mut ran = 0;
    let mut failed = 0;
    for jig in &selected {
        match runner::run_test(jig)? {
            None => {}
            Some(outcome) => {
                ran += 1;
                if outcome.status == 0 {
                    println!("ok    {}", jig.name);
                } else {
                    failed += 1;
                    println!("FAIL  {} (exit {})", jig.name, outcome.status);
                    if !outcome.stderr.trim().is_empty() {
                        for line in outcome.stderr.lines() {
                            println!("        {line}");
                        }
                    }
                }
            }
        }
    }
    println!("\n{ran} tested, {failed} failed");
    if failed > 0 {
        exit(1);
    }
    Ok(())
}

fn cmd_tree(path: Option<PathBuf>, budget: usize, depth: Option<usize>) -> Result<()> {
    let root = path.unwrap_or_else(|| PathBuf::from("."));
    let out = tree::render(
        &root,
        &tree::Options {
            budget,
            max_depth: depth,
        },
    )?;
    print!("{out}");
    Ok(())
}

fn cmd_changes(since: String) -> Result<()> {
    let out = changes::render(&PathBuf::from("."), &since)?;
    print!("{out}");
    Ok(())
}
