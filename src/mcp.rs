//! MCP server. Every jig — plus the built-in tools — is exposed as a first-class
//! typed tool over stdio, with `notifications/tools/list_changed` fired whenever
//! a jig file changes on disk, so an agent sees a tool the moment it is forged.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use eyre::Result;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::{NotificationContext, Peer, RequestContext};
use rmcp::transport::stdio;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Map, Value};

use crate::builtins::{changes, tree};
use crate::{discovery, runner, scaffold, schema};

#[derive(Clone)]
pub struct GabaritServer;

pub async fn serve() -> Result<()> {
    let service = GabaritServer.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}

impl GabaritServer {
    fn all_tools(&self) -> Vec<Tool> {
        let mut tools = builtin_tools();
        for jig in discovery::discover() {
            let desc = jig
                .description()
                .unwrap_or_else(|| format!("jig: {}", jig.name));
            let input_schema = Arc::new(schema::to_input_schema(&jig.spec));
            tools.push(Tool::new(jig.tool_name(), desc, input_schema));
        }
        tools
    }
}

impl ServerHandler for GabaritServer {
    fn get_info(&self) -> ServerInfo {
        let capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_tool_list_changed()
            .build();
        ServerInfo::new(capabilities)
            .with_server_info(Implementation::new("gabarit", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Project-local tools (jigs) forged by agents, plus built-ins (gabarit_tree, \
                 gabarit_changes, gabarit_new). Prefer an existing jig over hand-rolling a \
                 shell pipeline; forge a new jig with gabarit_new for anything you'd do twice.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.all_tools()))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let name = request.name.to_string();
        let args = request.arguments.unwrap_or_default();
        match name.as_str() {
            "gabarit_tree" => call_tree(&args),
            "gabarit_changes" => call_changes(&args),
            "gabarit_new" => call_new(&args),
            _ => call_jig(&name, &args),
        }
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        spawn_watcher(context.peer.clone());
    }
}

fn ok_text(s: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(s.into())])
}

fn err_text(s: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(s.into())])
}

fn call_jig(tool_name: &str, args: &Map<String, Value>) -> Result<CallToolResult, McpError> {
    let jigs = discovery::discover();
    let Some(jig) = jigs.into_iter().find(|j| j.tool_name() == tool_name) else {
        return Err(McpError::invalid_params(
            format!("unknown tool: {tool_name}"),
            None,
        ));
    };
    let argv = schema::json_args_to_argv(&jig.spec, args);
    match runner::run_captured(&jig, &argv) {
        Ok(outcome) => {
            let mut body = outcome.stdout;
            if !outcome.stderr.trim().is_empty() {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(&outcome.stderr);
            }
            if outcome.status == 0 {
                Ok(ok_text(body))
            } else {
                Ok(err_text(format!(
                    "jig exited with code {}\n{body}",
                    outcome.status
                )))
            }
        }
        Err(e) => Ok(err_text(e.to_string())),
    }
}

fn call_tree(args: &Map<String, Value>) -> Result<CallToolResult, McpError> {
    let path = args
        .get("path")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let budget = args.get("budget").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;
    let depth = args
        .get("depth")
        .and_then(|v| v.as_u64())
        .map(|d| d as usize);
    match tree::render(
        &path,
        &tree::Options {
            budget,
            max_depth: depth,
        },
    ) {
        Ok(out) => Ok(ok_text(out)),
        Err(e) => Ok(err_text(e.to_string())),
    }
}

fn call_changes(args: &Map<String, Value>) -> Result<CallToolResult, McpError> {
    let since = args
        .get("since")
        .and_then(|v| v.as_str())
        .unwrap_or("7d")
        .to_string();
    match changes::render(&PathBuf::from("."), &since) {
        Ok(out) => Ok(ok_text(out)),
        Err(e) => Ok(err_text(e.to_string())),
    }
}

fn call_new(args: &Map<String, Value>) -> Result<CallToolResult, McpError> {
    let Some(name) = args.get("name").and_then(|v| v.as_str()) else {
        return Err(McpError::invalid_params("name is required", None));
    };
    let spec = scaffold::NewJig {
        name: name.to_string(),
        description: args
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
        interpreter: args
            .get("interpreter")
            .and_then(|v| v.as_str())
            .unwrap_or("bash")
            .to_string(),
        content: args
            .get("content")
            .and_then(|v| v.as_str())
            .map(String::from),
    };
    match scaffold::create(&spec) {
        Ok(created) => Ok(ok_text(format!(
            "created jig '{}' at {}",
            created.name,
            created.path.display()
        ))),
        Err(e) => Ok(err_text(e.to_string())),
    }
}

fn builtin_tools() -> Vec<Tool> {
    vec![
        Tool::new(
            "gabarit_tree",
            "Token-dense, gitignore-aware map of a directory for orientation.",
            Arc::new(obj(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory to map (default: cwd)"},
                    "budget": {"type": "integer", "description": "Approx token budget (default 2000)"},
                    "depth": {"type": "integer", "description": "Max depth to expand"}
                }
            }))),
        ),
        Tool::new(
            "gabarit_changes",
            "Dense digest of recent git activity: commits, hot files, working-tree state.",
            Arc::new(obj(json!({
                "type": "object",
                "properties": {
                    "since": {"type": "string", "description": "Ref (e.g. HEAD~5) or duration (e.g. 7d); default 7d"}
                }
            }))),
        ),
        Tool::new(
            "gabarit_new",
            "Forge a new jig (a reusable, self-describing tool) into .gabarit/jigs.",
            Arc::new(obj(json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "description": "Jig name; ':' creates subdirs"},
                    "description": {"type": "string"},
                    "interpreter": {"type": "string", "description": "bash|python|node|ruby (default bash)"},
                    "content": {"type": "string", "description": "Full file contents including #USAGE/#GABARIT headers"}
                },
                "required": ["name"]
            }))),
        ),
    ]
}

fn obj(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

fn spawn_watcher(peer: Peer<RoleServer>) {
    use notify::{RecursiveMode, Watcher};

    let dirs = discovery::jig_dirs();
    if dirs.is_empty() {
        return;
    }
    let (tx, mut rx) = tokio::sync::mpsc::channel::<()>(16);
    let mut watcher =
        match notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = tx.blocking_send(());
            }
        }) {
            Ok(w) => w,
            Err(_) => return,
        };
    for (dir, _) in &dirs {
        let _ = watcher.watch(dir, RecursiveMode::Recursive);
    }

    tokio::spawn(async move {
        let _watcher = watcher; // keep the watcher alive for the task's lifetime
        while rx.recv().await.is_some() {
            // Debounce a burst of filesystem events.
            tokio::time::sleep(Duration::from_millis(300)).await;
            while rx.try_recv().is_ok() {}
            let _ = peer.notify_tool_list_changed().await;
        }
    });
}
