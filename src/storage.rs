use crate::{
    address::BluetoothAddress, bluez::DeviceInfo, paths::TrackerPaths, util::unique_addresses,
};
use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

pub use crate::storage_jsonl::read_jsonl;
use crate::{
    storage_jsonl::{append_jsonl_unlocked, read_jsonl_unlocked, write_jsonl_unlocked},
    storage_lock::acquire_storage_lock,
};

pub const SCHEMA_VERSION: u32 = 1;
const MAX_NOTE_CHARS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveState {
    pub schema_version: u32,
    pub device_address: BluetoothAddress,
    #[serde(default)]
    pub device_name: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub started_at: OffsetDateTime,
    pub start_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_note: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_note: Option<String>,
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

pub fn known_device_addresses(paths: &TrackerPaths) -> Result<Vec<BluetoothAddress>> {
    let mut addresses = read_jsonl::<ActiveState>(paths.actives_path())?
        .into_iter()
        .map(|active| active.device_address)
        .collect::<Vec<_>>();
    addresses.extend(
        read_jsonl::<SpanRecord>(paths.spans_path())?
            .into_iter()
            .map(|span| span.device_address),
    );
    Ok(unique_addresses(addresses))
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
        schema_version: SCHEMA_VERSION,
        device_address: device.address.clone(),
        device_name: device.name.clone(),
        started_at,
        start_source: source.as_ref().to_owned(),
        start_note: None,
        end_note: None,
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

    let spans = read_jsonl_unlocked::<SpanRecord>(&spans_path)?;
    if let Some(record) = completed_span_for_active(&spans, &active).cloned() {
        actives.remove(index);
        write_jsonl_unlocked(actives_path, actives)?;
        return Ok(DisconnectOutcome::Closed(record));
    }

    let duration_seconds = (ended_at - active.started_at).whole_seconds().max(0);
    let record = SpanRecord {
        schema_version: SCHEMA_VERSION,
        device_address: device.address.clone(),
        device_name: device.name.clone().or(active.device_name),
        started_at: active.started_at,
        ended_at,
        duration_seconds,
        start_source: active.start_source,
        end_source: source.as_ref().to_owned(),
        end_uncertain,
        start_note: active.start_note,
        end_note: active.end_note,
    };

    append_jsonl_unlocked(spans_path, &record, "span")?;
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

pub fn set_span_note(
    paths: &TrackerPaths,
    address: Option<&BluetoothAddress>,
    boundary: SpanBoundary,
    note: impl AsRef<str>,
) -> Result<NoteOutcome> {
    let note = normalize_note(note)?;
    let actives_path = paths.actives_path();
    let spans_path = paths.spans_path();
    let _lock = acquire_storage_lock(paths.state_dir())?;

    let mut actives = read_jsonl_unlocked::<ActiveState>(&actives_path)?;
    if let Some(active) = match address {
        Some(addr) => actives
            .iter_mut()
            .rev()
            .find(|active| active.device_address == *addr),
        None => actives.last_mut(),
    } {
        let address = active.device_address.clone();
        set_note_field(&mut active.start_note, &mut active.end_note, boundary, note);
        write_jsonl_unlocked(actives_path, actives)?;
        return Ok(NoteOutcome::ActiveSpan(address));
    }

    let mut spans = read_jsonl_unlocked::<SpanRecord>(&spans_path)?;
    if let Some(span) = match address {
        Some(addr) => spans
            .iter_mut()
            .rev()
            .find(|span| span.device_address == *addr),
        None => spans.last_mut(),
    } {
        let address = span.device_address.clone();
        set_note_field(&mut span.start_note, &mut span.end_note, boundary, note);
        write_jsonl_unlocked(spans_path, &spans)?;
        return Ok(NoteOutcome::LatestSpan(address));
    }

    match address {
        Some(address) => Err(anyhow!(
            "no active or completed spans for {address} to annotate"
        )),
        None => Err(anyhow!("no active or completed spans to annotate")),
    }
}

fn set_note_field(
    start_note: &mut Option<String>,
    end_note: &mut Option<String>,
    boundary: SpanBoundary,
    note: String,
) {
    match boundary {
        SpanBoundary::Start => *start_note = Some(note),
        SpanBoundary::End => *end_note = Some(note),
    }
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
            schema_version: SCHEMA_VERSION,
            device_address: device.address.clone(),
            device_name: device.name.clone(),
            started_at,
            ended_at,
            duration_seconds: (ended_at - started_at).whole_seconds().max(0),
            start_source: "test-connect".to_owned(),
            end_source: "test-disconnect".to_owned(),
            end_uncertain: false,
            start_note: None,
            end_note: None,
        }
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

        assert_eq!(record.duration_seconds, 600);
        assert!(!record.end_uncertain);
        assert!(read_jsonl::<ActiveState>(paths.actives_path())?.is_empty());
        assert_eq!(read_jsonl::<SpanRecord>(paths.spans_path())?, vec![record]);
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
        append_jsonl_unlocked(paths.spans_path(), &record, "span")?;

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
        append_jsonl_unlocked(paths.spans_path(), &record, "span")?;

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
    fn note_without_address_uses_latest_available_span() -> Result<()> {
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
        assert_eq!(record.duration_seconds, 60);
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
        Ok(())
    }

    #[test]
    fn empty_note_errors() {
        assert!(normalize_note(" \n\t ").is_err());
    }
}
