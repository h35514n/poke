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
pokes_per_day = 6
min_spacing_minutes = 45

[messages]
items = [
  "Drink some water.",
  "Stand up and stretch.",
  "Take a walk.",
  "Contemplate the orb."
]
```

The same starter config is also available at `assets/config.toml.sample`.

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

At the first tick of each local calendar day, `poke` generates `pokes_per_day`
scheduled times inside `[start_hour, end_hour)`. It divides the active window
into equal segments, picks one random timestamp per segment, and enforces
`min_spacing_minutes`.

For each generated poke, `poke` assigns one entry from `[messages].items`
using a shuffle-and-cycle strategy: the message list is shuffled once per day,
then assigned to poke slots in order, cycling back to the start if
`pokes_per_day` exceeds the number of messages. When `pokes_per_day` is at
least as large as the number of messages, every message is guaranteed to appear
at least once each day.

If multiple pokes are overdue when a tick runs, `poke` sends the earliest due
poke and drops the other missed overdue pokes after the send succeeds. Future
pokes stay pending.

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
