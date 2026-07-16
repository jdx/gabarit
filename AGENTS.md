# Working with gabarit

This project uses [gabarit](https://github.com/jdx/gabarit) to give coding agents a
persistent, project-local toolbelt. A **jig** is a single self-describing script
that declares its interface in `#USAGE`/`#GABARIT` comment headers.

## For agents working in a repo that has gabarit

**Before hand-rolling a shell pipeline, check for an existing jig:**

```sh
gabarit ls
```

If a jig already does what you need, run it — don't re-derive the pipeline:

```sh
gabarit run <name> [args...]
```

**Forge a jig for anything you'd do twice.** If you find yourself composing the
same multi-step `rg`/`awk`/`git` dance a second time, crystallize it:

```sh
gabarit new <name> --description "<one line>"
# then edit .gabarit/jigs/<name>.sh: add #USAGE arg/flag lines and the body
```

Guidelines for a good jig:

- **The description is the retrieval index.** Write a specific one-line
  `#GABARIT description` — it's how you (and other agents) will find it later.
- **Declare arguments with `#USAGE`.** Parsed values arrive as `$usage_<name>`
  env vars (with defaults applied) and as positional argv. This is what makes the
  jig a *typed* tool over MCP, not just a script.
- **Add a smoke test** with `#GABARIT test = "<args>"` and a fixture under
  `.gabarit/fixtures/` when practical, so CI can catch it when it rots.
- **Commit it.** A jig in git is reviewed like any other code and shared by the
  whole team and every agent they run.

## Paste-in stanza for your CLAUDE.md / AGENTS.md

> This repo uses gabarit. Run `gabarit ls` before writing an ad-hoc shell
> pipeline; if a jig fits, use `gabarit run`. Forge a new jig with `gabarit new`
> (or the `gabarit_new` MCP tool) for any multi-step operation you'd repeat.
