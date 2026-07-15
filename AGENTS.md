# Repository Guidelines

## Project Scope

This is a local, single-user project maintained for the owner's personal use. Favor simple, direct changes over production-oriented process and compatibility work. Unless explicitly requested, do not spend effort on migrations, backward compatibility, upgrade paths, rollout planning, or other concerns intended for external users or deployments.

## Project Structure & Module Organization

This repository is a Rust 2024 binary crate named `keychron-tracker`. `src/main.rs` is the entry point, and `src/cli.rs` parses commands. Tracking logic lives in `src/tracking.rs`, BlueZ/D-Bus integration in `src/bluez.rs`, service helpers in `src/service.rs`, persistence in `src/storage*.rs`, and reporting/display code in `src/report.rs` and `src/display.rs`. Tests are embedded as module tests in `src/*.rs`.

## Build, Test, and Development Commands

- `cargo build`: compile the debug binary.
- `cargo build --release`: build the optimized executable used in the README quick start.
- `cargo test`: run all unit tests.
- `cargo run -- discover`: run the tracker locally through Cargo.
- `cargo run -- report`: generate a report from the default state files.
- `cargo fmt --check`: verify Rust formatting before submitting changes.
- `cargo clippy --all-targets --all-features`: run lint checks when Clippy is installed.

Runtime commands that talk to Bluetooth require Linux with BlueZ and access to the user D-Bus session.

## Coding Style & Naming Conventions

Use standard `rustfmt` formatting and Rust 2024 idioms. Prefer small modules that keep domain responsibilities clear, matching the current layout. Use `snake_case` for functions, variables, modules, and tests; `PascalCase` for types; and `SCREAMING_SNAKE_CASE` for constants. Return `anyhow::Result` at command and integration boundaries where context-rich errors are useful. Prefer `impl AsRef<Path>`, `impl AsRef<str>`, `impl AsRef<OsStr>`, and `impl AsRef<[T]>` for simple borrowed input parameters when doing so improves caller flexibility without complicating the implementation. Very important to keep code clean, simple, minimal, and reasonably free of unnecessary abstractions, dependencies, duplication, and boilerplate.

## Testing Guidelines

Add focused unit tests next to the code they exercise inside `#[cfg(test)] mod tests`. Follow the existing descriptive test naming style, such as `parse_accepts_colon_separated_address`. Use `tempfile` for filesystem tests rather than writing to fixed paths. Run `cargo test` after behavior changes, and include tests for storage, parsing, span transitions, and reporting logic when those areas change.

## Commit & Pull Request Guidelines

Keep commit messages simple and descriptive enough to explain the change. Pull requests should include a clear summary, test results such as `cargo test`, and any operational impact for BlueZ, systemd user services, or state files under `~/.local/state/keychron-tracker/`.

## Security & Configuration Tips

Do not commit generated state files, Bluetooth addresses from private environments, or local systemd overrides. Treat data in `spans.jsonl` and `active.jsonl` as user activity history.

## Data & Schema Changes

State is JSONL under `~/.local/state/keychron-tracker/`. Because this is a local personal project, schema changes do not need migrations or backward compatibility unless explicitly requested. It is acceptable to clear or manually adjust local state when the schema changes.
