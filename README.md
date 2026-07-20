# Bluetooth Device Tracker

Tracks Bluetooth connection spans and battery observations for explicit Bluetooth
devices on Linux/BlueZ.

## Quick Start

```sh
cargo build --release
./target/release/keychron-tracker discover
./target/release/keychron-tracker service install --address AA:BB:CC:DD:EE:FF
systemctl --user status keychron-tracker
```

By default, storage is under `$XDG_STATE_HOME/keychron-tracker`, or
`~/.local/state/keychron-tracker` when `XDG_STATE_HOME` is unset. Tracked spans are
appended to:

```text
~/.local/state/keychron-tracker/spans.jsonl
```

Active, not-yet-closed spans and device state are stored at:

```text
~/.local/state/keychron-tracker/active.jsonl
```

Each non-empty line in `active.jsonl` is one currently active device span.

Use the global `--state-dir PATH` option to select another storage directory. The
filenames are fixed: both `spans.jsonl` and `active.jsonl` are always stored in that
directory and share one storage lock. For example:

```sh
keychron-tracker --state-dir /path/to/tracker-state report
```

When installing the systemd service, a relative state directory is resolved against
the directory where `service install` is run.

## Commands

```sh
keychron-tracker discover
keychron-tracker status
keychron-tracker status --address AA:BB:CC:DD:EE:FF
keychron-tracker watch --address AA:BB:CC:DD:EE:FF --address 11:22:33:44:55:66
keychron-tracker report
keychron-tracker report --address AA:BB:CC:DD:EE:FF
keychron-tracker note start --address AA:BB:CC:DD:EE:FF focused writing
keychron-tracker note end --address AA:BB:CC:DD:EE:FF coffee break
keychron-tracker battery set 55
keychron-tracker service install --address AA:BB:CC:DD:EE:FF --address 11:22:33:44:55:66
keychron-tracker service uninstall
```

`watch` is the long-running command used by the user-level systemd service. It listens
for BlueZ `org.bluez.Device1.Connected` and `org.bluez.Battery1.Percentage` changes
through one D-Bus match rule covering all BlueZ device paths. Connection changes trigger
a fresh query of BlueZ's current state; the possibly stale value carried by the signal is
not applied directly. It also queries connection state at startup, every 60 seconds, and
when the system wakes. Battery values are recorded only from live change signals; the
tracker does not poll devices or save BlueZ's cached startup value.

`battery set` records a manual percentage on the device's active span, or on its latest
completed span when the device is disconnected. Automatic and manual observations are
stored inside their corresponding entries in `active.jsonl` and `spans.jsonl`. Use
`--address` when more than one device is active.

`note start` and `note end` add short notes to the active span when one exists, or
to the latest completed span otherwise. Use `--address` when more than one device is
active.

System sleep, suspend, and hibernate are treated as device disconnect periods. When
systemd-logind emits `PrepareForSleep(true)`, the tracker closes any active span at
that timestamp. When the machine wakes, it resyncs BlueZ and starts a new span only if
the device is connected.
