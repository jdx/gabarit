# gabarit

> # ⚠️ SUPER WORK IN PROGRESS — IGNORE THIS ⚠️
>
> **This is an early, half-baked experiment. Do not use it. Do not depend on it.
> Do not file issues or PRs. Everything here — the name, the format, the CLI, the
> MCP surface — will change or disappear without notice.** It exists in public only
> so the author can poke at it. If you stumbled onto this repo: nothing to see here,
> please move along.

**Toolbelt for coding agents — forge, discover, and run project-local tools ("jigs").**

Coding agents re-derive the same multi-step shell work every session. Skills persist *instructions*; MCP servers are human-authored *capabilities*. Nothing lets an agent crystallize repeated work into a persistent, typed, discoverable tool.

`gabarit` (French for *jig* — the custom fixture a craftsman builds for a repeated operation) fills that gap. A **jig** is a single self-describing script that declares its own interface in comment headers. gabarit discovers jigs, validates their arguments, runs them, and exposes them to agents as first-class **MCP tools** with dynamic registration — so a tool an agent forges once is callable, by name and with a typed schema, forever after.

## What a jig looks like

One file, one jig. The interface lives in comment headers — `#USAGE` (reusing the [usage](https://usage.jdx.dev) spec) declares arguments; `#GABARIT` carries metadata.

```bash
#!/usr/bin/env bash
#GABARIT description = "Extract failing test names from a CI log"
#GABARIT created-by = "claude-code"
#GABARIT created-at = 2026-07-16
#GABARIT test = "../fixtures/ci-sample.log --json"
#USAGE arg "<logfile>" help="Path to the CI log to scan"
#USAGE flag "--json" help="Emit one JSON object per line instead of plain names"
set -euo pipefail

# Parsed values arrive as $usage_<name> env vars (defaults applied) and as argv.
grep -E '^FAIL' "$usage_logfile"
```

Drop it in `.gabarit/jigs/` and it's immediately runnable and immediately visible to any connected agent. Any language works — the shebang picks the interpreter, and no exec bit is required.

## Install

```sh
cargo install gabarit
```

## Use

```sh
gabarit new extract-fails            # scaffold a jig
gabarit ls                           # list discovered jigs
gabarit run extract-fails ci.log     # run one (args validated against its spec)
gabarit info extract-fails           # metadata + rendered help
gabarit test                         # run jigs' declared smoke tests (for CI)
```

Jigs are discovered from `.gabarit/jigs/` in the current directory and its ancestors (project scope), plus `~/.gabarit/jigs/` (global). Project jigs shadow global ones. Subdirectories become `:`-separated names (`ci/logs.sh` → `ci:logs`).

### Built-in tools

Two agent-oriented commands ship in the box, tuned for information-per-token rather than glanceability:

```sh
gabarit tree src/ --budget 1500      # token-dense, gitignore-aware repo map
gabarit changes --since 7d           # dense digest of recent git activity
```

`tree` aggregates repetitive directories, makes every elision explicit, and renders as deep as the token budget allows. `changes` summarizes commits, the files with the most churn, and working-tree state.

## Connect to an agent (MCP)

`gabarit mcp` is a stdio MCP server. Every jig plus the built-ins appear as typed tools, and the list live-reloads as jig files change — an agent sees a tool the moment it's forged.

Claude Code:

```sh
claude mcp add gabarit -- gabarit mcp
```

Any other MCP-speaking harness: run `gabarit mcp` over stdio. The server declares `tools.listChanged`, so clients that support it pick up new jigs without a restart. It also exposes `gabarit_new`, letting an agent forge a jig in one call without shell access.

## Smoke tests

A jig may declare a `#GABARIT test = "<args>"` line: one sample invocation, run from the jig's directory (so `../fixtures/...` resolves). `gabarit test` runs them and exits non-zero on failure — wire it into CI to catch jigs that have rotted. Tests are optional and are not run automatically; the CLI and MCP paths never gate on them.

## Roadmap

Not yet in v1: execution sandboxing, a "suggest" loop that mines session transcripts for repeated command shapes worth crystallizing, automatic hygiene/demotion of broken jigs, and multi-file jigs.

## License

MIT
