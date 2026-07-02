use crate::address::BluetoothAddress;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime, UtcOffset};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackerPaths {
    pub log_path: PathBuf,
    pub state_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveState {
    pub schema_version: u32,
    pub device_address: BluetoothAddress,
    pub device_name: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    pub start_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpanRecord {
    pub schema_version: u32,
    pub device_address: BluetoothAddress,
    pub device_name: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub ended_at: OffsetDateTime,
    pub duration_seconds: i64,
    pub start_source: String,
    pub end_source: String,
    pub end_uncertain: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectOutcome {
    Started,
    AlreadyActive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisconnectOutcome {
    Closed(SpanRecord),
    NoActiveSpan,
}

impl TrackerPaths {
    pub fn from_overrides(log_path: Option<PathBuf>, state_path: Option<PathBuf>) -> Result<Self> {
        let state_dir = default_state_dir()?;
        Ok(Self {
            log_path: log_path.unwrap_or_else(|| state_dir.join("spans.jsonl")),
            state_path: state_path.unwrap_or_else(|| state_dir.join("active.json")),
        })
    }
}

pub fn default_state_dir() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_STATE_HOME") {
        return Ok(PathBuf::from(path).join("keychron-tracker"));
    }

    let home = env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".local/state/keychron-tracker"))
}

pub fn default_user_systemd_dir() -> Result<PathBuf> {
    if let Some(path) = env::var_os("XDG_CONFIG_HOME") {
        return Ok(PathBuf::from(path).join("systemd/user"));
    }

    let home = env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/systemd/user"))
}

pub fn load_active(path: &Path) -> Result<Option<ActiveState>> {
    match fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("failed to parse active state {}", path.display()))
            .map(Some),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

pub fn load_spans(path: &Path) -> Result<Vec<SpanRecord>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err).with_context(|| format!("failed to open {}", path.display())),
    };

    let mut spans = Vec::new();
    for (line_number, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| {
            format!(
                "failed to read line {} from {}",
                line_number + 1,
                path.display()
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let span = serde_json::from_str(&line).with_context(|| {
            format!(
                "failed to parse line {} from {}",
                line_number + 1,
                path.display()
            )
        })?;
        spans.push(span);
    }
    Ok(spans)
}

pub fn mark_connected(
    paths: &TrackerPaths,
    address: &BluetoothAddress,
    name: Option<&str>,
    started_at: OffsetDateTime,
    source: &str,
) -> Result<ConnectOutcome> {
    if let Some(active) = load_active(&paths.state_path)? {
        if active.device_address != *address {
            return Err(anyhow!(
                "active state is for {}, but observed connect is for {}",
                active.device_address,
                address
            ));
        }
        return Ok(ConnectOutcome::AlreadyActive);
    }

    let active = ActiveState {
        schema_version: SCHEMA_VERSION,
        device_address: address.clone(),
        device_name: name.map(ToOwned::to_owned),
        started_at,
        start_source: source.to_owned(),
    };
    write_json_file(&paths.state_path, &active)?;
    Ok(ConnectOutcome::Started)
}

pub fn mark_disconnected(
    paths: &TrackerPaths,
    address: &BluetoothAddress,
    name: Option<&str>,
    ended_at: OffsetDateTime,
    source: &str,
    end_uncertain: bool,
) -> Result<DisconnectOutcome> {
    let Some(active) = load_active(&paths.state_path)? else {
        return Ok(DisconnectOutcome::NoActiveSpan);
    };

    if active.device_address != *address {
        return Err(anyhow!(
            "active state is for {}, but observed disconnect is for {}",
            active.device_address,
            address
        ));
    }

    let duration_seconds = (ended_at - active.started_at).whole_seconds().max(0);
    let record = SpanRecord {
        schema_version: SCHEMA_VERSION,
        device_address: address.clone(),
        device_name: active.device_name.or_else(|| name.map(ToOwned::to_owned)),
        started_at: active.started_at,
        ended_at,
        duration_seconds,
        start_source: active.start_source,
        end_source: source.to_owned(),
        end_uncertain,
    };

    ensure_parent_dir(&paths.log_path)?;
    append_span(&paths.log_path, &record)?;
    match fs::remove_file(&paths.state_path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to remove {}", paths.state_path.display()))
        }
    }

    Ok(DisconnectOutcome::Closed(record))
}

pub fn format_timestamp(value: OffsetDateTime) -> String {
    let value = UtcOffset::local_offset_at(value)
        .map(|offset| value.to_offset(offset))
        .unwrap_or(value);
    format_timestamp_value(value)
}

fn format_timestamp_value(value: OffsetDateTime) -> String {
    value
        .replace_nanosecond(0)
        .unwrap_or(value)
        .format(&Rfc3339)
        .unwrap_or_else(|_| value.unix_timestamp().to_string())
}

pub fn format_duration(total_seconds: i64) -> String {
    let total_seconds = total_seconds.max(0);
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn append_span(path: &Path, record: &SpanRecord) -> Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))?;
    serde_json::to_writer(&mut file, record)
        .with_context(|| format!("failed to write {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn write_json_file<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    ensure_parent_dir(path)?;
    let bytes = serde_json::to_vec_pretty(value)?;
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use time::macros::datetime;

    fn paths(temp: &TempDir) -> TrackerPaths {
        TrackerPaths {
            log_path: temp.path().join("spans.jsonl"),
            state_path: temp.path().join("active.json"),
        }
    }

    #[test]
    fn connected_then_disconnected_writes_one_span() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let address = BluetoothAddress::new("aa:bb:cc:dd:ee:ff");
        let started_at = datetime!(2026-06-28 12:00 UTC);
        let ended_at = datetime!(2026-06-28 12:10 UTC);

        assert_eq!(
            mark_connected(
                &paths,
                &address,
                Some("Keychron K3"),
                started_at,
                "test-connect"
            )?,
            ConnectOutcome::Started
        );
        assert_eq!(
            mark_connected(
                &paths,
                &address,
                Some("Keychron K3"),
                started_at,
                "test-connect"
            )?,
            ConnectOutcome::AlreadyActive
        );

        let outcome = mark_disconnected(
            &paths,
            &address,
            Some("Keychron K3"),
            ended_at,
            "test-disconnect",
            false,
        )?;
        let DisconnectOutcome::Closed(record) = outcome else {
            panic!("expected span closure");
        };
        assert_eq!(record.duration_seconds, 600);
        assert!(!record.end_uncertain);
        assert!(load_active(&paths.state_path)?.is_none());
        assert_eq!(load_spans(&paths.log_path)?, vec![record]);
        Ok(())
    }

    #[test]
    fn format_timestamp_omits_subsecond_precision() {
        let timestamp = datetime!(2026-06-28 12:00:01.123456789 UTC);
        assert!(!format_timestamp(timestamp).contains('.'));
    }

    #[test]
    fn format_timestamp_value_uses_offset_in_display_value() {
        let timestamp = datetime!(2026-06-28 12:00:01.123456789 -4);
        assert_eq!(
            format_timestamp_value(timestamp),
            "2026-06-28T12:00:01-04:00"
        );
    }

    #[test]
    fn disconnected_without_active_span_is_noop() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let address = BluetoothAddress::new("aa:bb:cc:dd:ee:ff");
        assert_eq!(
            mark_disconnected(
                &paths,
                &address,
                None,
                datetime!(2026-06-28 12:10 UTC),
                "test-disconnect",
                false,
            )?,
            DisconnectOutcome::NoActiveSpan
        );
        assert!(load_spans(&paths.log_path)?.is_empty());
        Ok(())
    }

    #[test]
    fn uncertain_restart_closure_is_marked() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let address = BluetoothAddress::new("aa:bb:cc:dd:ee:ff");

        mark_connected(
            &paths,
            &address,
            None,
            datetime!(2026-06-28 12:00 UTC),
            "startup-connected",
        )?;
        let outcome = mark_disconnected(
            &paths,
            &address,
            None,
            datetime!(2026-06-28 12:01 UTC),
            "startup-disconnected",
            true,
        )?;

        let DisconnectOutcome::Closed(record) = outcome else {
            panic!("expected span closure");
        };
        assert!(record.end_uncertain);
        assert_eq!(record.duration_seconds, 60);
        Ok(())
    }

    #[test]
    fn connected_with_active_state_for_other_address_errors() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let first_address = BluetoothAddress::new("aa:bb:cc:dd:ee:ff");
        let second_address = BluetoothAddress::new("11:22:33:44:55:66");

        mark_connected(
            &paths,
            &first_address,
            None,
            datetime!(2026-06-28 12:00 UTC),
            "first-connect",
        )?;

        let err = mark_connected(
            &paths,
            &second_address,
            None,
            datetime!(2026-06-28 12:01 UTC),
            "second-connect",
        )
        .unwrap_err();

        assert!(err.to_string().contains("active state is for"));
        Ok(())
    }
}
