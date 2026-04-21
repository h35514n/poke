# AGENTS.md

## Purpose

Build `poke`, a compact Rust CLI for macOS that runs as a per-user LaunchAgent and sends scheduled iMessage nudges via `imsg`.

The product contract lives in `docs/SPEC.md`. If this file and `SPEC.md` conflict, follow `SPEC.md` for behavior and this file for repo workflow.

## Scope

Implement only the `poke` CLI, LaunchAgent template, sample config, README, and tests. Do not add unrelated tooling, services, or infrastructure.

## Non-goals

Do not introduce:

- async runtime
- database
- HTTP server
- resident daemon loop
- shell-wrapper-based architecture
- unnecessary abstractions
- nonessential dependencies

This tool should remain a short-lived CLI invoked by launchd every 5 minutes.

## Technical constraints

- Use stable Rust.
- Keep dependencies minimal.
- Keep runtime behavior simple and auditable.
- Use XDG-compliant config and state paths.
- Use atomic state writes.
- Use an interprocess lock during `tick`.
- Use timezone-aware local wall-clock time.
- Invoke `imsg send --to DESTINATION --text MESSAGE` directly as a subprocess, not through a shell.
- Use absolute paths where launchd interaction is involved.

## Source of truth

Read `docs/SPEC.md` before making changes.

Key requirements that must remain true:

- per-user LaunchAgent, not LaunchDaemon
- `poke tick` is the scheduled entrypoint
- config in XDG config dir
- state and logs in XDG state dir
- one daily generated schedule per local day
- at most one message sent per tick
- failed sends do not dequeue pending pokes
- on successful overdue sends, send the earliest due poke and drop any other missed overdue pokes
- pending queue is replaced on new-day regeneration
- LaunchAgent uses `ProgramArguments`, `StartInterval=300`, `RunAtLoad=true`
- no `KeepAlive`

## File layout

Prefer this layout unless there is a strong reason to change it:

- `Cargo.toml`
- `src/main.rs`
- `src/cli.rs`
- `src/paths.rs`
- `src/config.rs`
- `src/state.rs`
- `src/schedule.rs`
- `src/tick.rs`
- `src/delivery.rs`
- `src/launchagent.rs`
- `src/util.rs`
- `assets/com.example.poke.plist.in`
- `README.md`

Keep modules small and explicit.

## Style

- Prefer straightforward code over clever code.
- Separate pure logic from IO where practical.
- Use small, explicit structs and functions.
- Keep serialization schemas human-readable and stable.
- Write clear operator-facing error messages.
- Avoid premature generalization.

## Tests

Add or maintain tests for:

- XDG path resolution and fallback behavior
- rejection of relative XDG env vars
- schedule generation count and bounds
- minimum spacing enforcement
- day rollover behavior
- due-poke detection
- dequeue on successful send
- dropping other missed overdue pokes only after a successful send
- preservation of pending queue on failed send
- `show` does not mutate state
- every configured message appears when `pokes_per_day >= messages.len()`

Run tests before considering a task complete.

## Commands

Useful commands:

- `cargo fmt`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- `cargo build --release`

If you add a command to the README, verify it matches the implemented CLI.

## Editing guidance

When implementing features:

1. read the relevant section of `docs/SPEC.md`
2. inspect existing module boundaries
3. make the smallest coherent change
4. add or update tests
5. keep docs and examples aligned with behavior

## LaunchAgent guidance

The generated plist must:

- install under `~/Library/LaunchAgents/`
- call the absolute `poke` binary path with `tick`
- log to files under the XDG state log directory
- avoid shell indirection unless absolutely necessary

Do not design around a system-wide daemon model.

## Definition of done

A change is complete when:

- it matches `docs/SPEC.md`
- `cargo fmt`, `cargo clippy`, and `cargo test` pass
- behavior remains simple and operator-friendly
- docs and templates are updated if needed
