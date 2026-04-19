Build a small Rust CLI called `poke` for macOS. It is a per-user background utility, installed as a LaunchAgent, not a LaunchDaemon, because it must run in the logged-in user session and invoke `imsg` through the user’s iMessage context. The LaunchAgent should use `ProgramArguments`, run every 300 seconds via `StartInterval`, and log stdout/stderr to files under the app’s state directory. Use Rust stable with `edition = "2024"`. Apple’s launchd guidance covers agents vs daemons and plist keys like `ProgramArguments`, `StartInterval`, `RunAtLoad`, `StandardOutPath`, and `StandardErrorPath`. The XDG base directory spec defines the defaults for config and state paths. Rust’s current stable project defaults use edition 2024. ([Apple Developer][1])

The tool should be a short-lived command invoked repeatedly by launchd, not a resident daemon process. Each invocation should perform one “tick” and then exit. Do not daemonize, fork into the background, or run an internal sleep loop. The core product behavior is: once per new local calendar day, generate a randomized schedule of poke times between configured start and end hours; on each tick, check whether the next poke is due; if not due, exit 0; if due, send exactly one message through `imsg`, remove it from the queue, persist state atomically, and exit 0. If multiple scheduled intervals are missed during sleep, launchd may coalesce wake events, so the application itself must decide how to handle missed pokes. ([Apple Developer][1])

Functional requirements:

- Maintain config in `$XDG_CONFIG_HOME/poke/config.toml`, defaulting to `~/.config/poke/config.toml` if the env var is unset or empty.
- Maintain state in `$XDG_STATE_HOME/poke/state.json`, defaulting to `~/.local/state/poke/state.json` if unset or empty.
- Store logs in `$XDG_STATE_HOME/poke/log/`.
- Use absolute XDG paths only; if an XDG env var is set to a relative path, ignore it and fall back to the default path.
- Assume the current user is logged in and has a working iMessage account.
- Use a user-configurable destination number and user-configurable list of messages.
- Run every 5 minutes, but only send within the configured local-time window.
- At the first tick of a new local day, generate exactly `pokes_per_day` poke times for that day.
- Randomization must be “distributed across the day,” not clustered naïve-uniform. Implement this by dividing the active window into `N` segments and choosing one random timestamp within each segment, then enforcing minimum spacing.
- On each tick, send at most one poke.
- Default missed-poke policy: drop missed pokes older than the current tick and send only the earliest pending poke that is due now or overdue within the same day. Make this configurable later, but do not implement multiple policies in v1 unless trivial.
- If `imsg` fails, do not dequeue the poke; log the failure and exit nonzero.
- If today’s schedule has not yet been generated and the current time is already after `end_hour`, still generate and persist the day’s schedule, then no-op on sending. This keeps state coherent.
- Pokes must never spill across days; a new day replaces the pending queue with a fresh schedule.

Non-functional requirements:

- Keep runtime dependencies minimal and the binary self-contained.
- Use atomic state writes.
- Use an interprocess lock so overlapping invocations cannot double-send.
- Use timezone-aware local wall-clock datetimes.
- Be robust across sleep/wake and DST transitions.
- Prefer simple, auditable code over abstraction-heavy architecture.

Deliverables:

1. A Rust crate producing a single binary `poke`.
2. An installable LaunchAgent plist template `com.example.poke.plist` with token substitution for the binary path, username/home path, and log locations.
3. A sample `config.toml`.
4. A `README.md` with install, load/unload, debug, and usage instructions.
5. A small test suite covering schedule generation, day rollover, active-window logic, and due-poke dequeue behavior.

CLI surface:

- `poke tick`
  Main scheduled entrypoint. Safe to run repeatedly.
- `poke init`
  Create config/state/log directories and write a default config if absent.
- `poke show`
  Print current resolved paths, today’s generated schedule, pending queue, and last sent event.
- `poke regen`
  Force-regenerate today’s schedule and overwrite pending pokes for today.
- `poke install-agent`
  Write the LaunchAgent plist to `~/Library/LaunchAgents/com.example.poke.plist`, filling in the current binary path and state log paths.
- `poke uninstall-agent`
  Remove the plist. Do not automatically unload it; print the needed `launchctl` command.
- `poke print-plist`
  Print the plist to stdout for packaging/debugging.

Suggested crate choices:

- `clap` for CLI parsing.
- `serde`, `serde_json`, and `toml` for config/state serialization.
- `chrono` or `time` plus `chrono-tz` only if needed; prefer as little timezone machinery as possible, but local wall-clock handling must be correct.
- `fs2` or similar for file locking.
- `anyhow` for top-level error handling.
- `rand` for schedule jitter.

Do not add a database. Do not use async. Do not add a background thread. Do not add network behavior.

Config schema in `config.toml`:

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

Config semantics:

- `start_hour` and `end_hour` are local-time wall-clock hours in 24-hour format.
- Treat the active window as `[start_hour, end_hour)`.
- Reject invalid configs with clear errors: `end_hour <= start_hour`, `pokes_per_day <= 0`, empty `messages.items`, invalid phone number string empty, negative spacing, nonexistent `imsg_path`.
- It is acceptable for `pokes_per_day` to exceed the number of unique messages; messages may repeat.
- Minimum spacing is a hard constraint. If it cannot be satisfied for the configured window, return a config error explaining that the window is too small for the requested density.

State schema in `state.json`:

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

State semantics:

- `last_schedule_date` is the local calendar date for which `pending` was last generated.
- `pending` is always sorted ascending by `at`.
- `sent` only needs to retain entries for the current day in v1; on new-day generation, clear it.
- `id` must be stable and deterministic within the generated day schedule, for example `YYYY-MM-DD-index`.
- State updates must be atomic: write temp file, fsync, rename.
- Lock the state file or sibling lockfile for the full duration of `tick`.

Tick algorithm:

1. Resolve XDG config/state paths.
2. Acquire lock.
3. Load config.
4. Load state if present, else initialize empty state.
5. Get current local datetime and local date.
6. If `state.last_schedule_date != today`, generate today’s schedule and replace `pending`; clear `sent`; save state.
7. If `now` is outside active window, exit 0.
8. Find the earliest pending poke with `at <= now`.
9. If none exists, exit 0.
10. Execute `imsg` as a subprocess using the configured path and destination.
11. On success, move that poke from `pending` to `sent`, save state, exit 0.
12. On failure, log stderr/stdout and exit nonzero without mutating `pending`.

Schedule generation algorithm:

- Build the day’s active interval from local date at `start_hour:00:00` through `end_hour:00:00`.
- Split the interval into `pokes_per_day` contiguous segments of approximately equal duration.
- For each segment, sample one timestamp uniformly inside that segment.
- Sort sampled times.
- Enforce `min_spacing_minutes`; if any pair violates spacing, retry generation up to a bounded number of attempts.
- If no valid schedule is found, return a deterministic error saying the configured density is infeasible.
- Assign each time a message by random selection from `messages.items`.
- Persist the resulting queue sorted by time.

Subprocess behavior:

- Invoke `imsg` directly, not through a shell.
- Pass destination and message as distinct arguments.
- Capture exit code, stdout, and stderr for logging.
- The exact `imsg` CLI should be abstracted in one function so it is easy to change if needed.
- Do not assume PATH contains Homebrew binaries inside launchd; use the absolute configured `imsg_path`.

LaunchAgent requirements:

- Install under `~/Library/LaunchAgents/com.example.poke.plist`.
- Use `ProgramArguments` with the absolute path to the `poke` binary and the `tick` subcommand.
- Use `StartInterval = 300`.
- Use `RunAtLoad = true`.
- Set `StandardOutPath` and `StandardErrorPath` under the state log directory.
- Do not use `KeepAlive`.
- Do not use a shell wrapper unless unavoidable.

Plist template shape:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
  <dict>
    <key>Label</key>
    <string>com.example.poke</string>

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

File layout:

```text
poke/
  Cargo.toml
  src/
    main.rs
    cli.rs
    paths.rs
    config.rs
    state.rs
    schedule.rs
    tick.rs
    delivery.rs
    launchagent.rs
    util.rs
  assets/
    com.example.poke.plist.in
  README.md
```

Testing requirements:

- Unit test XDG path resolution fallback behavior.
- Unit test rejection of relative XDG env var values.
- Unit test schedule generation count equals `pokes_per_day`.
- Unit test all generated times lie within the active window.
- Unit test minimum spacing enforcement.
- Unit test new-day rollover replaces pending queue.
- Unit test due-poke detection and dequeue.
- Unit test no-op before first due time.
- Unit test failed send preserves pending queue.
- Unit test `show` output does not mutate state.

Implementation notes for Codex:

- Prefer explicit structs and straightforward modules.
- Keep serialization schemas stable and human-editable.
- Make error messages operator-friendly.
- Avoid clever abstractions.
- Keep all filesystem and subprocess side effects behind thin functions so tests can isolate logic.
- Where feasible, separate pure logic from IO.

Definition of done:

- `cargo test` passes.
- `cargo build --release` produces a working `poke` binary.
- `poke init`, `poke show`, `poke regen`, and `poke tick` work locally.
- `poke install-agent` writes a valid plist.
- When the plist is loaded with `launchctl`, the binary runs every five minutes and sends due messages through `imsg`.
- All config/state/log files use XDG-compliant locations and defaults. ([Apple Developer][1])

[1]: https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html?utm_source=chatgpt.com "Creating Launch Daemons and Agents"
