# poke Specification

`poke` is a compact Rust CLI for macOS. It runs as a per-user LaunchAgent and sends scheduled iMessage nudges through `imsg` in the logged-in user's session.

This document is the behavior contract. Keep it small, concrete, and aligned with the implemented CLI.

## Runtime Model

- Build one binary: `poke`.
- Use stable Rust, edition 2024.
- Run as a LaunchAgent, not a LaunchDaemon.
- `launchd` invokes `poke tick` every 300 seconds.
- Each invocation does one short-lived tick and exits.
- Do not daemonize, fork, run a sleep loop, use async, add a database, or add network behavior.

## Paths

- Config: `$XDG_CONFIG_HOME/poke/config.toml`.
- Config fallback: `~/.config/poke/config.toml`.
- State: `$XDG_STATE_HOME/poke/state.json`.
- State fallback: `~/.local/state/poke/state.json`.
- Logs: `$XDG_STATE_HOME/poke/log/`.
- Lock file: `$XDG_STATE_HOME/poke/state.lock`.
- If `XDG_CONFIG_HOME` or `XDG_STATE_HOME` is unset, empty, or relative, use the fallback.
- `HOME` must be absolute.

## CLI

- `poke init`: create config, state, and log directories; write a starter config if absent.
- `poke tick`: scheduled entrypoint; safe to run repeatedly.
- `poke show`: print resolved paths, pending queue, and last sent event without mutating state.
- `poke regen`: regenerate today's schedule and replace today's pending queue.
- `poke install-agent`: write `~/Library/LaunchAgents/com.USER.poke.plist` and print the load command.
- `poke uninstall-agent`: remove the plist and print the unload command.
- `poke print-plist`: print the rendered plist.

## Config

```toml
[delivery]
destination = "+15555555555"
imsg_path = "/opt/homebrew/bin/imsg"

[schedule]
start_hour = 9
end_hour = 21
pokes_per_day = 6
min_spacing_minutes = 45

[messages]
items = [
  "Update openclaw context.",
  "Drink water.",
  "Stand up and stretch.",
  "Walk around for two minutes.",
  "Do ten air squats."
]
```

Validation:

- `delivery.destination` must not be empty.
- `delivery.imsg_path` must be absolute and must exist when loading config for runtime commands.
- `schedule.start_hour` must be `0..=23`.
- `schedule.end_hour` must be `1..=24`.
- `schedule.end_hour` must be greater than `schedule.start_hour`.
- `schedule.pokes_per_day` must be greater than zero.
- `schedule.min_spacing_minutes` must not be negative.
- `messages.items` must contain at least one non-empty message.
- The configured active window must be large enough for the requested poke count and minimum spacing.

The active window is local wall-clock time `[start_hour, end_hour)`.

## State

```json
{
  "last_schedule_date": "2026-04-19",
  "pending": [
    {
      "id": "2026-04-19-0",
      "at": "2026-04-19T09:35:00-04:00",
      "message": "Drink water."
    }
  ],
  "sent": [
    {
      "id": "2026-04-19-0",
      "scheduled_at": "2026-04-19T09:35:00-04:00",
      "sent_at": "2026-04-19T09:36:02-04:00",
      "message": "Drink water."
    }
  ]
}
```

State rules:

- `last_schedule_date` is the local date for which `pending` was generated.
- `pending` is sorted ascending by `at`.
- `sent` only needs to retain the current day.
- On new-day generation, replace `pending` and clear `sent`.
- Poke IDs are deterministic within the day: `YYYY-MM-DD-index`.
- State writes are atomic: write temp file, fsync, rename, then best-effort fsync the parent directory.
- `tick` holds the state lock for the full operation.

## Schedule Generation

- Generate exactly `pokes_per_day` pokes for the local date.
- Build the active interval from local `start_hour:00` to local `end_hour:00`.
- Split the interval into `pokes_per_day` contiguous segments.
- Sample one timestamp uniformly within each segment.
- Sort timestamps and enforce `min_spacing_minutes`.
- Retry boundedly; if no valid schedule is found, return a clear infeasible-density error.
- Assign messages by shuffling `messages.items` once, then cycling through that order.
- If `pokes_per_day >= messages.items.len()`, every configured message appears at least once.

## Tick Behavior

1. Resolve paths and ensure directories.
2. Acquire the state lock.
3. Load and validate config.
4. Load state or initialize empty state.
5. Get the current timezone-aware local datetime.
6. If `last_schedule_date != today`, generate today's schedule, replace `pending`, clear `sent`, and save state.
7. If now is outside the active window, exit 0.
8. Find the earliest pending poke with `at <= now`.
9. If none exists, exit 0.
10. Send exactly one message.
11. On success, move the sent poke to `sent`, drop any other pending pokes with `at <= now`, save state, and exit 0.
12. On failure, log stdout/stderr/status, preserve `pending`, do not append to `sent`, and exit nonzero.

If the first tick of a day happens after `end_hour`, still generate and persist that day's schedule, then no-op on sending.

## Delivery

- Invoke `imsg` directly, never through a shell.
- Use the configured absolute `imsg_path`.
- Current command shape: `imsg send --to DESTINATION --text MESSAGE`.
- Pass every argument separately.
- Capture exit status, stdout, and stderr for failure logging.
- Keep the exact subprocess call behind one small function.

## LaunchAgent

- Install under `~/Library/LaunchAgents/com.USER.poke.plist`.
- `Label` is `com.USER.poke`, where `USER` is the current user.
- Use `ProgramArguments` with the absolute `poke` binary path and `tick`.
- Use `StartInterval = 300`.
- Use `RunAtLoad = true`.
- Set `StandardOutPath` and `StandardErrorPath` under the state log directory.
- Do not use `KeepAlive`.
- Do not use shell wrappers.

Template:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>Label</key>
    <string>{{BUNDLE_ID}}</string>

    <key>ProgramArguments</key>
    <array>
      <string>{{BINARY_PATH}}</string>
      <string>tick</string>
    </array>

    <key>StartInterval</key>
    <integer>300</integer>

    <key>RunAtLoad</key>
    <true/>

    <key>StandardOutPath</key>
    <string>{{STATE_LOG_DIR}}/out.log</string>

    <key>StandardErrorPath</key>
    <string>{{STATE_LOG_DIR}}/err.log</string>
  </dict>
</plist>
```

## Files

```text
Cargo.toml
src/main.rs
src/cli.rs
src/paths.rs
src/config.rs
src/state.rs
src/schedule.rs
src/tick.rs
src/delivery.rs
src/launchagent.rs
src/util.rs
assets/com.example.poke.plist.in
assets/config.toml.sample
README.md
```

## Dependencies

Keep dependencies minimal. Current intended crates:

- `anyhow` for top-level errors.
- `chrono` for local wall-clock datetimes.
- `clap` for CLI parsing.
- `fs2` for file locking.
- `rand` for schedule jitter and message shuffle.
- `serde`, `serde_json`, and `toml` for config and state.

## Tests

Maintain tests for:

- XDG path fallback behavior.
- Rejection of relative XDG env vars.
- Schedule count and active-window bounds.
- Minimum spacing enforcement.
- All messages appearing when `pokes_per_day >= messages.len()`.
- New-day rollover replacing pending and clearing sent.
- No-op before first due time.
- Due-poke dequeue after successful send.
- Dropping other missed overdue pokes only after successful send.
- Failed send preserving pending queue.
- `show` output not mutating state.

## Done

A change is done when:

- Behavior matches this spec.
- `cargo fmt` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- `cargo test` passes.
- README and samples match the implemented CLI.
