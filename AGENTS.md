# AGENTS.md

Instructions for coding agents (Codex and others) working in this repository.

## Current task

Implement the specification in `docs/spec/coreutils-alignment/` — align with microsoft/coreutils and add common Linux commands. Read, in order:

1. `docs/spec/coreutils-alignment/README.md` — goals, non-goals, acceptance
2. `01-architecture.md`, `02-conventions.md` — binding contracts for every command
3. `commands.md` — the command list, phase and module of each command
4. `07-phases.md` — work order; **finish Phase 0 before any other phase**
5. The command specs (`03-commands-core.md`, `04-commands-extra.md`, `05-existing-gaps.md`) and `06-testing.md` as each phase needs them

## Hard rules

- The spec wins over guesses. Where it is silent, match GNU behavior on Ubuntu 24.04 with `LC_ALL=C TZ=UTC`. If the spec and GNU conflict, follow the spec. If something is ambiguous or impossible, record it in the "known differences" table of `07-phases.md` and in your phase report; do not silently diverge.
- New dependencies only from the allowlist in `01-architecture.md` §6. Never add `clap`, `fancy-regex`, or crates that need a C compiler.
- Never read or port GNU source code (GPL). uutils (MIT) may be consulted; ported code needs the attribution header and a `NOTICE.md` entry.
- New source files must stay at or below 800 lines; split modules instead.
- Tests touch only temporary directories. Never run destructive commands (`rm`, `mv`, `chmod`, `truncate`, `tar -x`, `sed -i`, `patch`) against paths outside a test sandbox.
- Keep the build green on Windows, Linux and macOS. Use `#[cfg(...)]` for platform code; do not leave a platform failing.
- Do not change behavior of existing commands except where `05-existing-gaps.md` says so.
- Before each commit run: `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`; on Linux also `AGENTCMD_GNU_DIFF=1 cargo test --locked --test gnu_diff`.
- Commit messages use conventional commits (`feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `ci:`). Do not push unless the maintainer asks.
- End each phase with the report format at the bottom of `07-phases.md`.
