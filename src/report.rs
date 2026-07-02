use crate::{
    display::{format_duration, format_timestamp},
    paths::TrackerPaths,
    storage::{load_active, load_spans, ActiveState},
};
use anyhow::Result;
use time::OffsetDateTime;

pub fn print_report(paths: &TrackerPaths) -> Result<()> {
    let observed_at = OffsetDateTime::now_utc();
    let active = load_active(&paths.state_path)?;
    let spans = load_spans(&paths.log_path)?;

    println!("Log: {}", paths.log_path.display());
    println!("Spans: {}", spans.len());

    if let Some(active) = active.as_ref() {
        println!();
        for line in current_span_lines(active, observed_at) {
            println!("{line}");
        }
    }

    if spans.is_empty() {
        return Ok(());
    }

    println!();
    println!("Recent spans:");
    for span in spans
        .iter()
        .rev()
        .take(10)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let marker = if span.end_uncertain { " uncertain" } else { "" };
        println!(
            "{} -> {}  {}{}",
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

fn current_span_lines(active: &ActiveState, observed_at: OffsetDateTime) -> Vec<String> {
    let elapsed = (observed_at - active.started_at).whole_seconds().max(0);
    let mut lines = vec![
        "Current span:".to_owned(),
        format!(
            "{} -> now  {}",
            format_timestamp(active.started_at),
            format_duration(elapsed)
        ),
    ];
    push_note_line(&mut lines, "start", active.start_note.as_deref());
    push_note_line(&mut lines, "end", active.end_note.as_deref());
    lines
}

fn print_note(label: &str, note: Option<&str>) {
    if let Some(note) = note {
        println!("  {label}: {note}");
    }
}

fn push_note_line(lines: &mut Vec<String>, label: &str, note: Option<&str>) {
    if let Some(note) = note {
        lines.push(format!("  {label}: {note}"));
    }
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
            device_address: BluetoothAddress::new("aa:bb:cc:dd:ee:ff"),
            device_name: Some("Keychron K3".to_owned()),
            started_at: datetime!(2026-06-28 12:00 UTC),
            start_source: "test-connect".to_owned(),
            start_note: Some("focused writing".to_owned()),
            end_note: Some("coffee break".to_owned()),
        };

        let lines = current_span_lines(&active, datetime!(2026-06-28 12:10 UTC));

        assert_eq!(lines[0], "Current span:");
        assert!(lines[1].ends_with(" -> now  10m 0s"));
        assert_eq!(lines[2], "  start: focused writing");
        assert_eq!(lines[3], "  end: coffee break");
    }
}
