# AGENTS: cargo-device

Guidance for AI coding assistants (Claude Code, Cursor, Codex) working in this repository.

## Entry Points

| Tool | Primary entry | Notes |
|------|---------------|-------|
| Claude Code | `CLAUDE.md` | Imports this file via `@AGENTS.md` |
| Cursor Agent | `AGENTS.md` + `.cursor/rules/*` | Cursor reads this file natively; rules extend by scope |
| OpenAI Codex | `AGENTS.md` | Codex discovery is `AGENTS.md`-first |
| task-master-ai | Task context via MCP | Run `next_task` as first action when starting work |

**Navigation**: [README](README.md)

---

## Project Identity

- **What**: `cargo-device` — a Cargo subcommand (`cargo device build|run|deploy|sync <device>`) that cross-compiles, deploys, and runs Rust binaries on embedded Linux devices
- **Architecture**: single Rust binary (`cargo-device`), installed on the host, invoked by cargo as `cargo device`
- **Config**: reads `[device.*]` tables from `.cargo/config.toml`; merges per-machine overrides from `.cargo/device.local.toml`
- **Core principle**: orchestrate existing system tools (`cargo`, `ssh`, `sftp`, `rsync`) — do not reimplement protocols in Rust
- **Platforms**: Linux, macOS, and Windows hosts; the device is always Linux. Anything that builds a shell command or touches a path must work from all three (see README → Windows)
- **Phase**: initial development (M1 in progress)
- **Task management**: task-master-ai (tag: `m1-core-mvp`)

---

## Project Principles

- **KISS**: simplest approach that works; no abstraction until a real requirement demands it
- **80/20**: cover the main use cases first; edge cases come later
- **Compose, don't reimplement**: shell out to `ssh`, `rsync`, `cargo` — no SSH or file-transfer protocol in Rust
- **Public-safe**: no hardcoded IPs, hostnames, SSH keys, or paths from real machines ever enter the codebase
- **Ship working increments**: ship and iterate; don't wait for perfect

---

## Non-Negotiables

- `anyhow` for all error handling (binary — `anyhow::Result` throughout; no typed error propagation)
- `tracing` for logging — never `println!` for operational output
- No `unwrap()` / `expect()` on runtime-failable paths
- Never hardcode SSH hosts, IPs, deploy paths, or key paths — always read from config
- No unused abstractions: add complexity only when a concrete requirement demands it
- `///` doc comments on all public items

---

## Agent Behavior

- **Ask when unclear**: if requirements are ambiguous, ask before implementing
- **Proactive suggestions**: at task end, note improvements, follow-ups, or risks observed
- **Stay in scope**: fix what was asked; don't refactor unrelated code in the same commit
- **Concept first**: for non-trivial tasks, verify approach before writing code

## Definition of Done

A task is **done** when all four hold — not before:

1. **Implemented** — code compiles clean (`cargo build`, clippy `-D warnings`, fmt)
2. **Tests written** — unit or integration tests covering the core behaviour; focus on the 20% of cases that catch 80% of bugs (happy path + the one failure mode most likely to regress); no test padding
3. **Tests pass** — `cargo test` exits 0 locally
4. **Docs updated** — README config schema, `///` doc comments on public items, and any relevant hardware/deployment docs reflect the change

What does **not** count as done: compiles but untested; tested but docs are stale; docs updated but tests missing. On-device / integration tests that require physical hardware are exempt — document the gap explicitly (what was tested, what wasn't, how to verify on hardware).

---

## Roadmap

See [README → Roadmap](README.md#roadmap) for the milestone table.

**Current milestone**: M1 — tasks tracked via task-master-ai (tag: `m1-core-mvp`).

---

## Git Conventions

- Branch: `feat/<slug>`, `fix/<slug>`, `docs/<slug>`
- Commits: Conventional Commits — `feat:`, `fix:`, `docs:`, `refactor:` (no task IDs in commit message headers)
- Task refs in trailer: `Refs: Task-4` or `Closes: Task-4`
- Keep commits small and focused

---

## Build & Test

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt --all -- --check
```

---

## Information Architecture (Single Source of Truth)

Never copy information across files — link to the source instead.

| SSoT | Owns |
|------|------|
| **README.md** | project description, problem statement, CLI examples, config schema, build backend table, roadmap |
| **AGENTS.md** | principles, non-negotiables, git conventions, build/test commands, agent behavior |
| **`///` doc comments** | implementation detail for any public item |

If a section in this file grows beyond ~4 lines, extract it to README or inline code comments.
New durable rule relevant to almost every task? Add here. Anything narrower? Use `.cursor/rules/rust.mdc` or inline comments.
