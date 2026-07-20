use crate::{address::BluetoothAddress, bluez::DeviceInfo, paths::TrackerPaths};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use std::fmt;
use time::OffsetDateTime;

pub use crate::storage_jsonl::read_jsonl;
use crate::{
    storage_jsonl::{read_jsonl_unlocked, write_jsonl_unlocked},
    storage_lock::acquire_storage_lock,
};

const MAX_NOTE_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BatterySource {
    BluezPropertiesChanged,
    Manual,
}

impl fmt::Display for BatterySource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::BluezPropertiesChanged => "bluez-properties-changed",
            Self::Manual => "manual",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatteryObservation {
    #[serde(with = "time::serde::rfc3339")]
    pub observed_at: OffsetDateTime,
    pub percentage: u8,
    pub source: BatterySource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatteryOutcome {
    ActiveSpan(BluetoothAddress),
    LatestSpan(BluetoothAddress),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveState {
    pub device_name: Option<String>,
    pub device_address: BluetoothAddress,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    pub start_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub battery_observations: Vec<BatteryObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpanRecord {
    pub device_name: Option<String>,
    pub device_address: BluetoothAddress,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub ended_at: OffsetDateTime,
    pub start_source: String,
    pub end_source: String,
    pub end_uncertain: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_note: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub battery_observations: Vec<BatteryObservation>,
}

impl SpanRecord {
    pub fn duration_seconds(&self) -> u64 {
        (self.ended_at - self.started_at).whole_seconds().max(0) as u64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectOutcome {
    Started,
    AlreadyActive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum DisconnectOutcome {
    Closed(SpanRecord),
    NoActiveSpan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanBoundary {
    Start,
    End,
}

impl SpanBoundary {
    pub fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::End => "end",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteOutcome {
    ActiveSpan(BluetoothAddress),
    LatestSpan(BluetoothAddress),
}

fn normalize_note(note: impl AsRef<str>) -> Result<String> {
    let note = note
        .as_ref()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if note.is_empty() {
        return Err(anyhow!("note cannot be empty"));
    }
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(anyhow!("note must be {MAX_NOTE_CHARS} characters or fewer"));
    }
    Ok(note)
}

pub fn mark_connected(
    paths: &TrackerPaths,
    device: &DeviceInfo,
    started_at: OffsetDateTime,
    source: impl AsRef<str>,
) -> Result<ConnectOutcome> {
    let actives_path = paths.actives_path();
    let _lock = acquire_storage_lock(paths.state_dir())?;

    let mut actives = read_jsonl_unlocked::<ActiveState>(&actives_path)?;
    if let Some(index) = actives
        .iter()
        .position(|active| active.device_address == device.address)
    {
        let spans = read_jsonl_unlocked::<SpanRecord>(paths.spans_path())?;
        if completed_span_for_active(&spans, &actives[index]).is_some() {
            actives.remove(index);
        } else {
            return Ok(ConnectOutcome::AlreadyActive);
        }
    }

    actives.push(ActiveState {
        device_address: device.address.clone(),
        device_name: device.name.clone(),
        started_at,
        start_source: source.as_ref().to_owned(),
        start_note: None,
        end_note: None,
        battery_observations: Vec::new(),
    });

    write_jsonl_unlocked(actives_path, actives)?;

    Ok(ConnectOutcome::Started)
}

pub fn mark_disconnected(
    paths: &TrackerPaths,
    device: &DeviceInfo,
    ended_at: OffsetDateTime,
    source: impl AsRef<str>,
    end_uncertain: bool,
) -> Result<DisconnectOutcome> {
    let actives_path = paths.actives_path();
    let spans_path = paths.spans_path();

    let _lock = acquire_storage_lock(paths.state_dir())?;
    let mut actives = read_jsonl_unlocked::<ActiveState>(&actives_path)?;

    let Some(index) = actives
        .iter()
        .position(|active| active.device_address == device.address)
    else {
        return Ok(DisconnectOutcome::NoActiveSpan);
    };

    let active = actives[index].clone();

    let mut spans = read_jsonl_unlocked::<SpanRecord>(&spans_path)?;
    if let Some(record) = completed_span_for_active(&spans, &active).cloned() {
        actives.remove(index);
        write_jsonl_unlocked(actives_path, actives)?;
        return Ok(DisconnectOutcome::Closed(record));
    }

    let record = SpanRecord {
        device_address: device.address.clone(),
        device_name: device.name.clone().or(active.device_name),
        started_at: active.started_at,
        ended_at,
        start_source: active.start_source,
        end_source: source.as_ref().to_owned(),
        end_uncertain,
        start_note: active.start_note,
        end_note: active.end_note,
        battery_observations: active.battery_observations,
    };

    spans.push(record.clone());
    write_jsonl_unlocked(spans_path, spans)?;
    actives.remove(index);
    write_jsonl_unlocked(actives_path, actives)?;

    Ok(DisconnectOutcome::Closed(record))
}

fn completed_span_for_active<'a>(
    spans: &'a [SpanRecord],
    active: &ActiveState,
) -> Option<&'a SpanRecord> {
    spans.iter().rev().find(|span| {
        span.device_address == active.device_address && span.started_at == active.started_at
    })
}

pub fn record_battery_observation(
    paths: &TrackerPaths,
    address: Option<&BluetoothAddress>,
    observation: BatteryObservation,
) -> Result<BatteryOutcome> {
    let actives_path = paths.actives_path();
    let spans_path = paths.spans_path();

    let _lock = acquire_storage_lock(paths.state_dir())?;
    let mut actives = read_jsonl_unlocked::<ActiveState>(&actives_path)?;

    let active_index = match address {
        Some(address) => actives
            .iter()
            .position(|active| active.device_address == *address),
        None if actives.len() == 1 => Some(0),
        None if actives.len() > 1 => {
            return Err(anyhow!(
                "multiple active spans; specify an address to record a battery level"
            ));
        }
        None => None,
    };

    if let Some(index) = active_index {
        let active = &mut actives[index];
        let selected_address = active.device_address.clone();
        active.battery_observations.push(observation);
        write_jsonl_unlocked(actives_path, actives)?;
        return Ok(BatteryOutcome::ActiveSpan(selected_address));
    }

    let mut spans = read_jsonl_unlocked::<SpanRecord>(&spans_path)?;
    let span_index = match address {
        Some(address) => spans
            .iter()
            .rposition(|span| span.device_address == *address),
        None => spans.len().checked_sub(1),
    };

    if let Some(index) = span_index {
        let span = &mut spans[index];
        let selected_address = span.device_address.clone();
        span.battery_observations.push(observation);
        write_jsonl_unlocked(spans_path, spans)?;
        return Ok(BatteryOutcome::LatestSpan(selected_address));
    }

    match address {
        Some(address) => Err(anyhow!(
            "no active or completed spans for {address} to record a battery level"
        )),
        None => Err(anyhow!(
            "no active or completed spans to record a battery level"
        )),
    }
}

pub fn record_manual_battery(
    paths: &TrackerPaths,
    address: Option<&BluetoothAddress>,
    percentage: u8,
    observed_at: OffsetDateTime,
) -> Result<BatteryOutcome> {
    let observation = BatteryObservation {
        observed_at,
        percentage,
        source: BatterySource::Manual,
    };
    record_battery_observation(paths, address, observation)
}

pub fn set_span_note(
    paths: &TrackerPaths,
    address: Option<&BluetoothAddress>,
    boundary: SpanBoundary,
    note: impl AsRef<str>,
) -> Result<NoteOutcome> {
    let note = normalize_note(note)?;
    let set_note_field = |start_note: &mut Option<String>,
                          end_note: &mut Option<String>,
                          boundary: SpanBoundary,
                          note: String| match boundary {
        SpanBoundary::Start => *start_note = Some(note),
        SpanBoundary::End => *end_note = Some(note),
    };
    let actives_path = paths.actives_path();
    let spans_path = paths.spans_path();
    let _lock = acquire_storage_lock(paths.state_dir())?;

    let mut actives = read_jsonl_unlocked::<ActiveState>(&actives_path)?;
    let active_index = match address {
        Some(address) => actives
            .iter()
            .position(|active| active.device_address == *address),
        None if actives.len() == 1 => Some(0),
        None if actives.len() > 1 => {
            return Err(anyhow!(
                "multiple active spans; specify an address to add a note"
            ));
        }
        None => None,
    };

    if let Some(index) = active_index {
        let active = &mut actives[index];
        let selected_address = active.device_address.clone();
        set_note_field(&mut active.start_note, &mut active.end_note, boundary, note);
        write_jsonl_unlocked(actives_path, actives)?;
        return Ok(NoteOutcome::ActiveSpan(selected_address));
    }

    let mut spans = read_jsonl_unlocked::<SpanRecord>(&spans_path)?;
    let span_index = match address {
        Some(address) => spans
            .iter()
            .rposition(|span| span.device_address == *address),
        None => spans.len().checked_sub(1),
    };

    if let Some(index) = span_index {
        let span = &mut spans[index];
        let selected_address = span.device_address.clone();
        set_note_field(&mut span.start_note, &mut span.end_note, boundary, note);
        write_jsonl_unlocked(spans_path, &spans)?;
        return Ok(NoteOutcome::LatestSpan(selected_address));
    }

    match address {
        Some(address) => Err(anyhow!(
            "no active or completed spans for {address} to annotate"
        )),
        None => Err(anyhow!("no active or completed spans to annotate")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::TrackerPaths;
    use tempfile::TempDir;
    use time::macros::datetime;

    fn paths(temp: &TempDir) -> TrackerPaths {
        TrackerPaths::new(temp.path())
    }

    fn device(address: impl AsRef<str>, name: Option<String>) -> DeviceInfo {
        let address = BluetoothAddress::new_unchecked(address);
        DeviceInfo {
            path: format!("/org/bluez/hci0/dev_{}", address.as_str().replace(':', "_")),
            address,
            name,
            connected: true,
        }
    }

    fn completed_span(
        device: &DeviceInfo,
        started_at: OffsetDateTime,
        ended_at: OffsetDateTime,
    ) -> SpanRecord {
        SpanRecord {
            device_address: device.address.clone(),
            device_name: device.name.clone(),
            started_at,
            ended_at,
            start_source: "test-connect".to_owned(),
            end_source: "test-disconnect".to_owned(),
            end_uncertain: false,
            start_note: None,
            end_note: None,
            battery_observations: Vec::new(),
        }
    }

    #[test]
    fn span_duration_is_never_negative() {
        let device = device("aa:bb:cc:dd:ee:ff", None);
        let record = completed_span(
            &device,
            datetime!(2026-06-28 12:10 UTC),
            datetime!(2026-06-28 12:00 UTC),
        );

        assert_eq!(record.duration_seconds(), 0);
    }

    #[test]
    fn connected_then_disconnected_writes_one_span() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = device("aa:bb:cc:dd:ee:ff", Some("Keychron K3".to_owned()));
        let started_at = datetime!(2026-06-28 12:00 UTC);
        let ended_at = datetime!(2026-06-28 12:10 UTC);

        assert_eq!(
            mark_connected(&paths, &device, started_at, "test-connect")?,
            ConnectOutcome::Started
        );
        assert_eq!(
            mark_connected(&paths, &device, started_at, "test-connect")?,
            ConnectOutcome::AlreadyActive
        );

        let DisconnectOutcome::Closed(record) =
            mark_disconnected(&paths, &device, ended_at, "test-disconnect", false)?
        else {
            panic!("expected span closure");
        };

        assert_eq!(record.duration_seconds(), 600);
        assert!(!record.end_uncertain);
        assert!(read_jsonl::<ActiveState>(paths.actives_path())?.is_empty());
        assert_eq!(read_jsonl::<SpanRecord>(paths.spans_path())?, vec![record]);
        let contents = std::fs::read_to_string(paths.spans_path())?;
        assert!(!contents.contains("duration_seconds"));
        assert!(!contents.contains("schema_version"));
        Ok(())
    }

    #[test]
    fn interrupted_disconnect_retry_removes_active_without_duplicate_span() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = device("aa:bb:cc:dd:ee:ff", Some("Keychron K3".to_owned()));
        let started_at = datetime!(2026-06-28 12:00 UTC);
        let record = completed_span(&device, started_at, datetime!(2026-06-28 12:10 UTC));

        mark_connected(&paths, &device, started_at, "test-connect")?;
        write_jsonl_unlocked(paths.spans_path(), [&record])?;

        assert_eq!(
            mark_disconnected(
                &paths,
                &device,
                datetime!(2026-06-28 12:11 UTC),
                "retry-disconnect",
                true,
            )?,
            DisconnectOutcome::Closed(record.clone())
        );
        assert!(read_jsonl::<ActiveState>(paths.actives_path())?.is_empty());
        assert_eq!(read_jsonl::<SpanRecord>(paths.spans_path())?, vec![record]);
        Ok(())
    }

    #[test]
    fn connected_startup_recovers_interrupted_close_and_starts_new_span() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = device("aa:bb:cc:dd:ee:ff", Some("Keychron K3".to_owned()));
        let stale_started_at = datetime!(2026-06-28 12:00 UTC);
        let record = completed_span(&device, stale_started_at, datetime!(2026-06-28 12:10 UTC));
        let new_started_at = datetime!(2026-06-28 12:20 UTC);

        mark_connected(&paths, &device, stale_started_at, "test-connect")?;
        write_jsonl_unlocked(paths.spans_path(), [&record])?;

        assert_eq!(
            mark_connected(&paths, &device, new_started_at, "startup-connected")?,
            ConnectOutcome::Started
        );
        let actives = read_jsonl::<ActiveState>(paths.actives_path())?;
        assert_eq!(actives.len(), 1);
        assert_eq!(actives[0].device_address, device.address);
        assert_eq!(actives[0].started_at, new_started_at);
        assert_eq!(actives[0].start_source, "startup-connected");
        assert_eq!(read_jsonl::<SpanRecord>(paths.spans_path())?, vec![record]);
        Ok(())
    }

    #[test]
    fn interrupted_close_matching_uses_address_and_start_timestamp() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let tracked_device = device("aa:bb:cc:dd:ee:ff", None);
        let other_device = device("11:22:33:44:55:66", None);
        let started_at = datetime!(2026-06-28 12:00 UTC);
        let wrong_address =
            completed_span(&other_device, started_at, datetime!(2026-06-28 12:05 UTC));
        let wrong_start = completed_span(
            &tracked_device,
            datetime!(2026-06-28 11:00 UTC),
            datetime!(2026-06-28 11:05 UTC),
        );

        mark_connected(&paths, &tracked_device, started_at, "test-connect")?;
        write_jsonl_unlocked(paths.spans_path(), [&wrong_address, &wrong_start])?;

        let DisconnectOutcome::Closed(record) = mark_disconnected(
            &paths,
            &tracked_device,
            datetime!(2026-06-28 12:10 UTC),
            "test-disconnect",
            false,
        )?
        else {
            panic!("expected span closure");
        };

        assert_eq!(record.device_address, tracked_device.address);
        assert_eq!(record.started_at, started_at);
        let spans = read_jsonl::<SpanRecord>(paths.spans_path())?;
        assert_eq!(spans, vec![wrong_address, wrong_start, record]);
        Ok(())
    }

    #[test]
    fn active_state_is_written_as_jsonl_entries() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = device("aa:bb:cc:dd:ee:ff", Some("Keychron K3".to_owned()));

        mark_connected(
            &paths,
            &device,
            datetime!(2026-06-28 12:00 UTC),
            "test-connect",
        )?;
        set_span_note(
            &paths,
            Some(&device.address),
            SpanBoundary::Start,
            "focused writing",
        )?;

        let contents = std::fs::read_to_string(paths.actives_path())?;
        let lines = contents.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 1);

        let active: ActiveState = serde_json::from_str(lines[0])?;
        assert_eq!(active.device_address, device.address);
        assert_eq!(active.start_note.as_deref(), Some("focused writing"));
        assert!(!contents.contains("schema_version"));
        Ok(())
    }

    #[test]
    fn active_span_notes_are_carried_to_closed_span() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = device("aa:bb:cc:dd:ee:ff", Some("Keychron K3".to_owned()));

        mark_connected(
            &paths,
            &device,
            datetime!(2026-06-28 12:00 UTC),
            "test-connect",
        )?;
        assert_eq!(
            set_span_note(
                &paths,
                Some(&device.address),
                SpanBoundary::Start,
                "  focused   writing  "
            )?,
            NoteOutcome::ActiveSpan(device.address.clone())
        );
        assert_eq!(
            set_span_note(
                &paths,
                Some(&device.address),
                SpanBoundary::End,
                "wrapped up"
            )?,
            NoteOutcome::ActiveSpan(device.address.clone())
        );

        mark_disconnected(
            &paths,
            &device,
            datetime!(2026-06-28 12:10 UTC),
            "test-disconnect",
            false,
        )?;
        let spans = read_jsonl::<SpanRecord>(paths.spans_path())?;
        assert_eq!(spans[0].start_note.as_deref(), Some("focused writing"));
        assert_eq!(spans[0].end_note.as_deref(), Some("wrapped up"));
        Ok(())
    }

    #[test]
    fn note_without_active_span_updates_latest_completed_span() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = device("aa:bb:cc:dd:ee:ff", None);

        mark_connected(
            &paths,
            &device,
            datetime!(2026-06-28 12:00 UTC),
            "test-connect",
        )?;
        mark_disconnected(
            &paths,
            &device,
            datetime!(2026-06-28 12:10 UTC),
            "test-disconnect",
            false,
        )?;

        assert_eq!(
            set_span_note(
                &paths,
                Some(&device.address),
                SpanBoundary::End,
                "coffee break"
            )?,
            NoteOutcome::LatestSpan(device.address.clone())
        );
        let spans = read_jsonl::<SpanRecord>(paths.spans_path())?;
        assert_eq!(spans[0].end_note.as_deref(), Some("coffee break"));
        Ok(())
    }

    #[test]
    fn note_without_address_uses_only_active_device_or_latest_completed_span() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = device("aa:bb:cc:dd:ee:ff", None);

        mark_connected(
            &paths,
            &device,
            datetime!(2026-06-28 12:00 UTC),
            "test-connect",
        )?;
        assert_eq!(
            set_span_note(&paths, None, SpanBoundary::Start, "active note")?,
            NoteOutcome::ActiveSpan(device.address.clone())
        );
        mark_disconnected(
            &paths,
            &device,
            datetime!(2026-06-28 12:10 UTC),
            "test-disconnect",
            false,
        )?;
        assert_eq!(
            set_span_note(&paths, None, SpanBoundary::End, "completed note")?,
            NoteOutcome::LatestSpan(device.address.clone())
        );

        let spans = read_jsonl::<SpanRecord>(paths.spans_path())?;
        assert_eq!(spans[0].start_note.as_deref(), Some("active note"));
        assert_eq!(spans[0].end_note.as_deref(), Some("completed note"));
        Ok(())
    }

    #[test]
    fn note_without_address_rejects_multiple_active_devices() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let first = device("aa:bb:cc:dd:ee:ff", None);
        let second = device("11:22:33:44:55:66", None);
        mark_connected(
            &paths,
            &first,
            datetime!(2026-06-28 12:00 UTC),
            "first-connect",
        )?;
        mark_connected(
            &paths,
            &second,
            datetime!(2026-06-28 12:01 UTC),
            "second-connect",
        )?;

        assert!(set_span_note(&paths, None, SpanBoundary::Start, "wrong").is_err());
        let actives = read_jsonl::<ActiveState>(paths.actives_path())?;
        assert!(actives.iter().all(|active| active.start_note.is_none()));
        Ok(())
    }

    #[test]
    fn addressed_note_missing_active_span_does_not_update_latest_active() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let first = device("aa:bb:cc:dd:ee:ff", None);
        let second = device("11:22:33:44:55:66", None);
        let missing = BluetoothAddress::new_unchecked("77:88:99:AA:BB:CC");

        mark_connected(
            &paths,
            &first,
            datetime!(2026-06-28 12:00 UTC),
            "first-connect",
        )?;
        mark_connected(
            &paths,
            &second,
            datetime!(2026-06-28 12:01 UTC),
            "second-connect",
        )?;

        assert!(set_span_note(&paths, Some(&missing), SpanBoundary::Start, "wrong").is_err());
        let actives = read_jsonl::<ActiveState>(paths.actives_path())?;
        assert!(actives.iter().all(|active| active.start_note.is_none()));
        assert!(actives.iter().all(|active| active.end_note.is_none()));
        Ok(())
    }

    #[test]
    fn addressed_note_missing_completed_span_does_not_update_latest_completed() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let first = device("aa:bb:cc:dd:ee:ff", None);
        let second = device("11:22:33:44:55:66", None);
        let missing = BluetoothAddress::new_unchecked("77:88:99:AA:BB:CC");

        mark_connected(
            &paths,
            &first,
            datetime!(2026-06-28 12:00 UTC),
            "first-connect",
        )?;
        mark_disconnected(
            &paths,
            &first,
            datetime!(2026-06-28 12:10 UTC),
            "first-disconnect",
            false,
        )?;
        mark_connected(
            &paths,
            &second,
            datetime!(2026-06-28 12:20 UTC),
            "second-connect",
        )?;
        mark_disconnected(
            &paths,
            &second,
            datetime!(2026-06-28 12:30 UTC),
            "second-disconnect",
            false,
        )?;

        assert!(set_span_note(&paths, Some(&missing), SpanBoundary::End, "wrong").is_err());
        let spans = read_jsonl::<SpanRecord>(paths.spans_path())?;
        assert!(spans.iter().all(|span| span.start_note.is_none()));
        assert!(spans.iter().all(|span| span.end_note.is_none()));
        Ok(())
    }

    #[test]
    fn disconnected_without_active_span_is_noop() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = device("aa:bb:cc:dd:ee:ff", None);

        assert_eq!(
            mark_disconnected(
                &paths,
                &device,
                datetime!(2026-06-28 12:10 UTC),
                "test-disconnect",
                false,
            )?,
            DisconnectOutcome::NoActiveSpan
        );
        assert!(read_jsonl::<SpanRecord>(paths.spans_path())?.is_empty());
        Ok(())
    }

    #[test]
    fn uncertain_restart_closure_is_marked() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = device("aa:bb:cc:dd:ee:ff", None);

        mark_connected(
            &paths,
            &device,
            datetime!(2026-06-28 12:00 UTC),
            "startup-connected",
        )?;
        let DisconnectOutcome::Closed(record) = mark_disconnected(
            &paths,
            &device,
            datetime!(2026-06-28 12:01 UTC),
            "startup-disconnected",
            true,
        )?
        else {
            panic!("expected span closure");
        };

        assert!(record.end_uncertain);
        assert_eq!(record.duration_seconds(), 60);
        Ok(())
    }

    #[test]
    fn multiple_devices_can_be_active_simultaneously() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let first = device("aa:bb:cc:dd:ee:ff", None);
        let second = device("11:22:33:44:55:66", None);

        mark_connected(
            &paths,
            &first,
            datetime!(2026-06-28 12:00 UTC),
            "first-connect",
        )?;
        assert_eq!(
            mark_connected(
                &paths,
                &second,
                datetime!(2026-06-28 12:01 UTC),
                "second-connect",
            )?,
            ConnectOutcome::Started
        );

        let actives = read_jsonl::<ActiveState>(paths.actives_path())?;
        assert!(
            actives
                .iter()
                .any(|active| active.device_address == first.address)
        );
        assert!(
            actives
                .iter()
                .any(|active| active.device_address == second.address)
        );
        Ok(())
    }

    #[test]
    fn disconnecting_one_device_leaves_other_device_active() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let first = device("aa:bb:cc:dd:ee:ff", None);
        let second = device("11:22:33:44:55:66", None);

        mark_connected(
            &paths,
            &first,
            datetime!(2026-06-28 12:00 UTC),
            "first-connect",
        )?;
        mark_connected(
            &paths,
            &second,
            datetime!(2026-06-28 12:01 UTC),
            "second-connect",
        )?;
        mark_disconnected(
            &paths,
            &first,
            datetime!(2026-06-28 12:10 UTC),
            "first-disconnect",
            false,
        )?;

        let actives = read_jsonl::<ActiveState>(paths.actives_path())?;
        assert!(
            !actives
                .iter()
                .any(|active| active.device_address == first.address)
        );
        assert!(
            actives
                .iter()
                .any(|active| active.device_address == second.address)
        );
        let spans = read_jsonl::<SpanRecord>(paths.spans_path())?;
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].device_address, first.address);
        Ok(())
    }

    #[test]
    fn legacy_spans_without_notes_still_load() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        std::fs::write(
            paths.spans_path(),
            concat!(
                r#"{"schema_version":1,"device_address":"AA:BB:CC:DD:EE:FF","#,
                r#""device_name":"Keychron K3","started_at":"2026-06-28T12:00:00Z","#,
                r#""ended_at":"2026-06-28T12:10:00Z","duration_seconds":600,"#,
                r#""start_source":"test","end_source":"test","end_uncertain":false}"#,
                "\n"
            ),
        )?;

        let spans = read_jsonl::<SpanRecord>(paths.spans_path())?;
        assert_eq!(spans.len(), 1);
        assert!(spans[0].start_note.is_none());
        assert!(spans[0].end_note.is_none());
        assert!(spans[0].battery_observations.is_empty());
        Ok(())
    }

    #[test]
    fn battery_observations_are_carried_to_closed_span() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = device("aa:bb:cc:dd:ee:ff", None);
        let automatic = BatteryObservation {
            observed_at: datetime!(2026-06-28 12:05 UTC),
            percentage: 80,
            source: BatterySource::BluezPropertiesChanged,
        };

        mark_connected(
            &paths,
            &device,
            datetime!(2026-06-28 12:00 UTC),
            "test-connect",
        )?;
        assert_eq!(
            record_battery_observation(&paths, Some(&device.address), automatic.clone())?,
            BatteryOutcome::ActiveSpan(device.address.clone())
        );
        assert_eq!(
            record_manual_battery(
                &paths,
                Some(&device.address),
                75,
                datetime!(2026-06-28 12:06 UTC),
            )?,
            BatteryOutcome::ActiveSpan(device.address.clone())
        );

        let DisconnectOutcome::Closed(span) = mark_disconnected(
            &paths,
            &device,
            datetime!(2026-06-28 12:10 UTC),
            "test-disconnect",
            false,
        )?
        else {
            panic!("expected span closure");
        };

        assert_eq!(span.battery_observations.len(), 2);
        assert_eq!(span.battery_observations[0], automatic);
        assert_eq!(span.battery_observations[1].percentage, 75);
        assert_eq!(span.battery_observations[1].source, BatterySource::Manual);
        assert!(read_jsonl::<ActiveState>(paths.actives_path())?.is_empty());
        Ok(())
    }

    #[test]
    fn manual_battery_without_address_updates_latest_completed_span() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = device("aa:bb:cc:dd:ee:ff", None);

        mark_connected(
            &paths,
            &device,
            datetime!(2026-06-28 12:00 UTC),
            "test-connect",
        )?;
        mark_disconnected(
            &paths,
            &device,
            datetime!(2026-06-28 12:10 UTC),
            "test-disconnect",
            false,
        )?;

        assert_eq!(
            record_manual_battery(&paths, None, 50, datetime!(2026-06-28 12:20 UTC),)?,
            BatteryOutcome::LatestSpan(device.address.clone())
        );
        let spans = read_jsonl::<SpanRecord>(paths.spans_path())?;
        assert_eq!(spans[0].battery_observations.len(), 1);
        assert_eq!(spans[0].battery_observations[0].percentage, 50);
        Ok(())
    }

    #[test]
    fn addressed_battery_requires_a_matching_span() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let address = BluetoothAddress::new_unchecked("aa:bb:cc:dd:ee:ff");
        let observation = BatteryObservation {
            observed_at: datetime!(2026-06-28 12:05 UTC),
            percentage: 80,
            source: BatterySource::BluezPropertiesChanged,
        };

        assert!(record_battery_observation(&paths, Some(&address), observation).is_err());
        assert!(read_jsonl::<ActiveState>(paths.actives_path())?.is_empty());
        assert!(read_jsonl::<SpanRecord>(paths.spans_path())?.is_empty());
        Ok(())
    }

    #[test]
    fn battery_without_address_uses_only_active_device() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = device("aa:bb:cc:dd:ee:ff", None);
        mark_connected(
            &paths,
            &device,
            datetime!(2026-06-28 12:00 UTC),
            "test-connect",
        )?;

        let outcome = record_battery_observation(
            &paths,
            None,
            BatteryObservation {
                observed_at: datetime!(2026-06-28 12:05 UTC),
                percentage: 80,
                source: BatterySource::Manual,
            },
        )?;

        assert_eq!(outcome, BatteryOutcome::ActiveSpan(device.address.clone()));
        let actives = read_jsonl::<ActiveState>(paths.actives_path())?;
        assert_eq!(actives[0].battery_observations[0].percentage, 80);
        Ok(())
    }

    #[test]
    fn battery_without_address_rejects_multiple_active_devices() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let first = device("aa:bb:cc:dd:ee:ff", None);
        let second = device("11:22:33:44:55:66", None);
        mark_connected(
            &paths,
            &first,
            datetime!(2026-06-28 12:00 UTC),
            "first-connect",
        )?;
        mark_connected(
            &paths,
            &second,
            datetime!(2026-06-28 12:01 UTC),
            "second-connect",
        )?;

        assert!(
            record_battery_observation(
                &paths,
                None,
                BatteryObservation {
                    observed_at: datetime!(2026-06-28 12:05 UTC),
                    percentage: 80,
                    source: BatterySource::Manual,
                },
            )
            .is_err()
        );
        let actives = read_jsonl::<ActiveState>(paths.actives_path())?;
        assert!(
            actives
                .iter()
                .all(|active| active.battery_observations.is_empty())
        );
        Ok(())
    }

    #[test]
    fn battery_without_address_uses_latest_completed_span_when_none_are_active() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let first = device("aa:bb:cc:dd:ee:ff", None);
        let second = device("11:22:33:44:55:66", None);
        for (device, started_at, ended_at) in [
            (
                &first,
                datetime!(2026-06-28 12:00 UTC),
                datetime!(2026-06-28 12:10 UTC),
            ),
            (
                &second,
                datetime!(2026-06-28 12:20 UTC),
                datetime!(2026-06-28 12:30 UTC),
            ),
        ] {
            mark_connected(&paths, device, started_at, "test-connect")?;
            mark_disconnected(&paths, device, ended_at, "test-disconnect", false)?;
        }

        let outcome = record_battery_observation(
            &paths,
            None,
            BatteryObservation {
                observed_at: datetime!(2026-06-28 12:40 UTC),
                percentage: 50,
                source: BatterySource::Manual,
            },
        )?;

        assert_eq!(outcome, BatteryOutcome::LatestSpan(second.address.clone()));
        let spans = read_jsonl::<SpanRecord>(paths.spans_path())?;
        assert!(spans[0].battery_observations.is_empty());
        assert_eq!(spans[1].battery_observations[0].percentage, 50);
        Ok(())
    }

    #[test]
    fn manual_battery_requires_a_span() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let address = BluetoothAddress::new_unchecked("aa:bb:cc:dd:ee:ff");

        assert!(
            record_manual_battery(&paths, Some(&address), 50, datetime!(2026-06-28 12:00 UTC))
                .is_err()
        );
        Ok(())
    }

    #[test]
    fn battery_sources_use_stable_json_names() -> Result<()> {
        let automatic = BatteryObservation {
            observed_at: datetime!(2026-06-28 12:05 UTC),
            percentage: 80,
            source: BatterySource::BluezPropertiesChanged,
        };
        let json = serde_json::to_string(&automatic)?;
        assert!(json.contains(r#""source":"bluez-properties-changed""#));

        let manual = json.replace("bluez-properties-changed", "manual");
        let observation: BatteryObservation = serde_json::from_str(&manual)?;
        assert_eq!(observation.source, BatterySource::Manual);
        Ok(())
    }

    #[test]
    fn empty_note_errors() {
        assert!(normalize_note(" \n\t ").is_err());
    }
}
