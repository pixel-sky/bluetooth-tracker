use crate::storage::{format_duration, format_timestamp, load_spans, TrackerPaths};
use anyhow::Result;

pub fn print_report(paths: &TrackerPaths) -> Result<()> {
    let spans = load_spans(&paths.log_path)?;
    let total_seconds: i64 = spans.iter().map(|span| span.duration_seconds).sum();

    println!("Log: {}", paths.log_path.display());
    println!("Spans: {}", spans.len());
    println!("Total connected time: {}", format_duration(total_seconds));

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
    }

    Ok(())
}
