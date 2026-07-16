# gabarit

**Toolbelt for coding agents — forge, discover, and run project-local tools ("jigs").**

> ⚠️ **v0.0.x is a placeholder.** gabarit is under active development; the real v0.1 is landing shortly. This early release exists to reserve the name.

## The idea

Coding agents re-derive the same multi-step shell work every session. Skills persist *instructions*; MCP servers are human-authored *capabilities*. Nothing lets an agent crystallize repeated work into a persistent, typed, discoverable tool.

`gabarit` (French for *jig* — the custom fixture a craftsman builds for a repeated operation) fills that gap. A **jig** is a single self-describing script that declares its own interface in comment headers. gabarit discovers jigs, validates their arguments, runs them, and exposes them to agents as first-class **MCP tools** with dynamic registration — so a tool an agent forges once is callable, by name and with a typed schema, forever after.

## Status

Coming in v0.1:

- `gabarit new / ls / run / info / test` — create and run jigs
- `gabarit mcp` — stdio MCP server that surfaces every jig as a typed tool, live-reloaded as jigs change
- Built-in tools: `tree` (token-dense repo map), `changes` (git activity digest)

## License

MIT
