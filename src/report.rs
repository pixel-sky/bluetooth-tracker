use crate::{
    address::BluetoothAddress,
    display::{format_duration, format_timestamp},
    paths::TrackerPaths,
    storage::{ActiveState, SpanRecord},
    storage_jsonl::read_jsonl_unlocked,
    storage_lock::acquire_storage_lock,
};
use anyhow::Result;
use time::OffsetDateTime;

fn device_label(address: &BluetoothAddress, name: Option<impl AsRef<str>>) -> String {
    match name {
        Some(name) if !name.as_ref().is_empty() => format!("{} ({address})", name.as_ref()),
        None => address.to_string(),
        Some(_) => address.to_string(),
    }
}

fn push_note_line(lines: &mut Vec<String>, label: impl AsRef<str>, note: Option<impl AsRef<str>>) {
    if let Some(note) = note {
        lines.push(format!("  {}: {}", label.as_ref(), note.as_ref()));
    }
}

fn current_span_lines(active: &ActiveState, observed_at: OffsetDateTime) -> Vec<String> {
    let elapsed = (observed_at - active.started_at).whole_seconds().max(0);
    let mut lines = vec![
        device_label(&active.device_address, active.device_name.as_deref()),
        format!(
            "  {} -> now  {}",
            format_timestamp(active.started_at),
            format_duration(elapsed)
        ),
    ];
    push_note_line(&mut lines, "start", active.start_note.as_deref());
    push_note_line(&mut lines, "end", active.end_note.as_deref());
    lines
}

fn print_note(label: impl AsRef<str>, note: Option<impl AsRef<str>>) {
    if let Some(note) = note {
        println!("  {}: {}", label.as_ref(), note.as_ref());
    }
}

pub fn print_report(paths: &TrackerPaths, addresses: impl AsRef<[BluetoothAddress]>) -> Result<()> {
    let observed_at = OffsetDateTime::now_utc();

    let is_selected = |address: &BluetoothAddress| {
        addresses.as_ref().is_empty()
            || addresses
                .as_ref()
                .iter()
                .any(|selected| selected == address)
    };

    let _lock = acquire_storage_lock(paths.state_dir())?;
    let actives = read_jsonl_unlocked::<ActiveState>(paths.actives_path())?
        .into_iter()
        .filter(|active| is_selected(&active.device_address))
        .collect::<Vec<_>>();
    let spans = read_jsonl_unlocked::<SpanRecord>(paths.spans_path())?
        .into_iter()
        .filter(|span| is_selected(&span.device_address))
        .collect::<Vec<_>>();
    drop(_lock);

    println!("Log: {}", paths.spans_path().display());
    println!("Spans: {}", spans.len());

    if !actives.is_empty() {
        println!();
        println!("Current spans:");
        for active in actives {
            for line in current_span_lines(&active, observed_at) {
                println!("{line}");
            }
        }
    }

    if spans.is_empty() {
        return Ok(());
    }

    println!();
    println!("Recent spans:");
    for span in &spans[spans.len().saturating_sub(10)..] {
        let marker = if span.end_uncertain { " uncertain" } else { "" };
        println!(
            "{}  {} -> {}  {}{}",
            device_label(&span.device_address, span.device_name.as_deref()),
            format_timestamp(span.started_at),
            format_timestamp(span.ended_at),
            format_duration(span.duration_seconds),
            marker
        );
        print_note("start", span.start_note.as_deref());
        print_note("end", span.end_note.as_deref());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::BluetoothAddress;
    use time::macros::datetime;

    #[test]
    fn current_span_lines_show_elapsed_time_and_notes() {
        let active = ActiveState {
            schema_version: 1,
            device_address: BluetoothAddress::new_unchecked("aa:bb:cc:dd:ee:ff"),
            device_name: Some("Keychron K3".to_owned()),
            started_at: datetime!(2026-06-28 12:00 UTC),
            start_source: "test-connect".to_owned(),
            start_note: Some("focused writing".to_owned()),
            end_note: Some("coffee break".to_owned()),
        };

        let lines = current_span_lines(&active, datetime!(2026-06-28 12:10 UTC));

        assert_eq!(lines[0], "Keychron K3 (AA:BB:CC:DD:EE:FF)");
        assert!(lines[1].ends_with(" -> now  10m 0s"));
        assert_eq!(lines[2], "  start: focused writing");
        assert_eq!(lines[3], "  end: coffee break");
    }
}
