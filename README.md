poke
====

`poke` is a compact macOS CLI that runs as a per-user LaunchAgent and sends
scheduled iMessage nudges with `imsg`.

Every five minutes, `launchd` runs `poke tick`, and each invocation sends at
most one due message.

Build
-----

```sh
cargo build --release
```

The release binary is `target/release/poke`.

Configure
---------

Create directories and a starter config:

```sh
poke init
```

The config is written to:

```text
$XDG_CONFIG_HOME/poke/config.toml
```

If `XDG_CONFIG_HOME` is unset, empty, or relative, `poke` uses:

```text
~/.config/poke/config.toml
```

Edit the config before loading the LaunchAgent:

```toml
[delivery]
destination = "+15555555555"
imsg_path = "/opt/homebrew/bin/imsg"

[schedule]
start_hour = 9
end_hour = 21
random_min_spacing_minutes = 45

[schedule.random_per_day]
default = 6      # baseline count for any weekday without an override
jitter = 2       # optional symmetric ± jitter applied each day; omit for none

[schedule.random_per_day.weekday]
saturday = 3
sunday = 3

[random]
items = [
  { text = "Update openclaw context.", category = "focus" },
  { text = "Drink water.", category = "hydration" },
  { text = "Stand up and stretch.", category = "mobility" },
  { text = "Walk around for two minutes.", category = "movement" },
  { text = "Do ten air squats.", category = "movement" }
]

[scheduled]
items = [
  { time = "15:00", text = "Send the afternoon check-in.", category = "fixed" }
]

[intervals]
items = [
  { every_minutes = 60, text = "Drink water.", category = "hydration" }
]
```

The same starter config is also available at `assets/config.toml.sample`.

`random.items` is the random message pool. Items can also remain a plain
string list. Plain strings are normalized to the default category `"default"`.

`schedule.random_per_day` is a table. `default` is the baseline number of random
pokes per day; optional `weekday.{monday..sunday}` keys override the baseline
for specific days; optional `jitter` applies a symmetric ± uniform integer offset
to the selected baseline each day, clamped to at least one. The maximum possible
daily count must fit the window under `random_min_spacing_minutes`; otherwise
config load fails with a clear error.

`scheduled.items` is optional. These are explicit daily messages delivered at
the configured local wall-clock time. They do not count toward the random count,
do not affect `random_min_spacing_minutes`, and can send outside the active window.
Use `"HH:MM"` times such as `"15:00"`; friendlier inputs such as `"3:00pm"` are
also accepted.

`intervals.items` is optional. These are fixed messages generated at
`start_hour`, then every `every_minutes` while the local time is before
`end_hour`. Intervals must be at least 5 minutes because `launchd` runs
`poke tick` every 5 minutes. Interval messages do not count toward the random
count, do not affect `random_min_spacing_minutes`, and stand outside random
rotation and recent-history logic.

`imsg_path` must be absolute. `poke tick` calls:

```sh
imsg send --to PHONE --text MESSAGE
```

State and Logs
--------------

State is stored at:

```text
$XDG_STATE_HOME/poke/state.json
```

If `XDG_STATE_HOME` is unset, empty, or relative, `poke` uses:

```text
~/.local/state/poke/state.json
```

Logs are stored under:

```text
$XDG_STATE_HOME/poke/log/
```

Commands
--------

```sh
poke init            # Create directories and a starter config
poke tick            # Run one scheduled tick
poke show            # Show resolved paths and current state
poke regen           # Force-regenerate today’s schedule
poke print-plist     # Print the LaunchAgent plist
poke install-agent   # Install the LaunchAgent
poke uninstall-agent # Uninstall the LaunchAgent
```

LaunchAgent Management
----------------------

Loading:

```sh
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.$USER.poke.plist
```

Unloading:

```sh
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.$USER.poke.plist
```

Behavior
--------

At the first tick of each local calendar day, `poke` picks the day's random
count — the weekday override (or `default`) plus a uniform integer in
`[-jitter, +jitter]`, clamped to at least one — and generates that many
random scheduled times inside `[start_hour, end_hour)`. It divides the active
window into equal segments, picks one random timestamp per segment, and enforces
`random_min_spacing_minutes`.

For each generated poke, `poke` chooses a category sequence that prefers
least-recently-used categories, avoids back-to-back category repeats when an
alternative exists, and carries a short recent-send history across day
boundaries. Within each category, it chooses the least-recently-used message,
again avoiding immediate repeats when possible.

When the day's random count is at least as large as the number of configured random
messages, `poke` still guarantees that every configured random message appears at
least once that day before it repeats any of them.

Explicit `scheduled.items` and `intervals.items` are added to the same pending
queue for the day, but they stand outside the random rotation and
recent-history logic. Scheduled messages can send outside the active window;
interval messages are active-window-bound.

If multiple pokes are overdue when a tick runs, `poke` sends the earliest due
poke and drops the other missed overdue pokes after the send succeeds. Future
pokes stay pending. If interval items overlap at the same time, only the first
overdue message is sent; stagger intervals if every interval item must be
delivered.

If `imsg` fails, `poke` exits nonzero and preserves the pending queue.

Debug
-----

Check launchd state:

```sh
launchctl print gui/$(id -u)/com.$USER.poke
```

Read logs:

```sh
tail -n 100 ~/.local/state/poke/log/out.log
tail -n 100 ~/.local/state/poke/log/err.log
```

Run checks:

```sh
cargo fmt
cargo clippy --all-targets --all-features -- -D warnings
cargo test
```
