# Keychron Tracker

Tracks Bluetooth connection spans for one Keychron keyboard on Linux/BlueZ.

## Quick Start

```sh
cargo build --release
./target/release/keychron-tracker discover
./target/release/keychron-tracker service install --address AA:BB:CC:DD:EE:FF
systemctl --user status keychron-tracker
```

Tracked spans are appended to:

```text
~/.local/state/keychron-tracker/spans.jsonl
```

The active, not-yet-closed span is stored at:

```text
~/.local/state/keychron-tracker/active.json
```

## Commands

```sh
keychron-tracker discover
keychron-tracker status --address AA:BB:CC:DD:EE:FF
keychron-tracker watch --address AA:BB:CC:DD:EE:FF
keychron-tracker report
keychron-tracker service install --address AA:BB:CC:DD:EE:FF
keychron-tracker service uninstall
```

`watch` is the long-running command used by the user-level systemd service. It listens
for BlueZ `org.bluez.Device1.Connected` changes through D-Bus.

System sleep, suspend, and hibernate are treated as keyboard disconnect periods. When
systemd-logind emits `PrepareForSleep(true)`, the tracker closes any active span at
that timestamp. When the machine wakes, it resyncs BlueZ and starts a new span only if
the keyboard is connected.
