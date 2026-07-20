use crate::{
    address::BluetoothAddress,
    display::{format_duration, format_timestamp},
    paths::TrackerPaths,
    storage::{ActiveState, BatteryObservation, SpanRecord},
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

struct BatteryReportEntry<'a> {
    address: &'a BluetoothAddress,
    name: Option<&'a str>,
    observation: &'a BatteryObservation,
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
    let battery_observations = collect_battery_observations(&actives, &spans);

    println!("Log: {}", paths.spans_path().display());
    println!("Spans: {}", spans.len());
    println!("Battery observations: {}", battery_observations.len());
    let print_note = |label: &str, note: Option<&str>| {
        if let Some(note) = note {
            println!("  {label}: {note}");
        }
    };

    if !actives.is_empty() {
        println!();
        println!("Current spans:");
        for active in &actives {
            let elapsed = (observed_at - active.started_at).whole_seconds().max(0) as u64;
            println!(
                "{}",
                device_label(&active.device_address, active.device_name.as_deref())
            );
            println!(
                "  {} -> now  {}",
                format_timestamp(active.started_at),
                format_duration(elapsed)
            );
            print_note("start", active.start_note.as_deref());
            print_note("end", active.end_note.as_deref());
        }
    }

    if !spans.is_empty() {
        println!();
        println!("Recent spans:");
        for span in &spans[spans.len().saturating_sub(10)..] {
            let marker = if span.end_uncertain { " uncertain" } else { "" };
            println!(
                "{}  {} -> {}  {}{}",
                device_label(&span.device_address, span.device_name.as_deref()),
                format_timestamp(span.started_at),
                format_timestamp(span.ended_at),
                format_duration(span.duration_seconds()),
                marker
            );
            print_note("start", span.start_note.as_deref());
            print_note("end", span.end_note.as_deref());
        }
    }

    if !battery_observations.is_empty() {
        println!();
        println!("Recent battery observations:");
        for entry in &battery_observations[battery_observations.len().saturating_sub(10)..] {
            println!(
                "{}",
                battery_observation_line(entry.address, entry.name, entry.observation)
            );
        }
    }

    Ok(())
}

fn collect_battery_observations<'a>(
    actives: &'a [ActiveState],
    spans: &'a [SpanRecord],
) -> Vec<BatteryReportEntry<'a>> {
    let mut observations = actives
        .iter()
        .flat_map(|active| {
            active
                .battery_observations
                .iter()
                .map(move |observation| BatteryReportEntry {
                    address: &active.device_address,
                    name: active.device_name.as_deref(),
                    observation,
                })
        })
        .chain(spans.iter().flat_map(|span| {
            span.battery_observations
                .iter()
                .map(move |observation| BatteryReportEntry {
                    address: &span.device_address,
                    name: span.device_name.as_deref(),
                    observation,
                })
        }))
        .collect::<Vec<_>>();
    observations.sort_by_key(|entry| entry.observation.observed_at);
    observations
}

fn battery_observation_line(
    address: &BluetoothAddress,
    name: Option<&str>,
    observation: &BatteryObservation,
) -> String {
    format!(
        "{}  {}  {}%  {}",
        device_label(address, name),
        format_timestamp(observation.observed_at),
        observation.percentage,
        observation.source
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::BluetoothAddress;
    use crate::storage::BatterySource;
    use time::macros::datetime;

    #[test]
    fn battery_observation_line_shows_device_time_percentage_and_source() {
        let address = BluetoothAddress::new_unchecked("aa:bb:cc:dd:ee:ff");
        let observation = BatteryObservation {
            observed_at: datetime!(2026-06-28 12:05 UTC),
            percentage: 55,
            source: BatterySource::Manual,
        };

        assert_eq!(
            battery_observation_line(&address, Some("Keychron K3"), &observation),
            format!(
                "Keychron K3 (AA:BB:CC:DD:EE:FF)  {}  55%  manual",
                format_timestamp(observation.observed_at)
            )
        );
    }

    #[test]
    fn battery_observations_are_flattened_in_timestamp_order() {
        let address = BluetoothAddress::new_unchecked("aa:bb:cc:dd:ee:ff");
        let active = ActiveState {
            device_address: address.clone(),
            device_name: Some("Keychron K3".to_owned()),
            started_at: datetime!(2026-06-28 12:10 UTC),
            start_source: "test".to_owned(),
            start_note: None,
            end_note: None,
            battery_observations: vec![BatteryObservation {
                observed_at: datetime!(2026-06-28 12:15 UTC),
                percentage: 55,
                source: BatterySource::Manual,
            }],
        };
        let span = SpanRecord {
            device_address: address,
            device_name: Some("Keychron K3".to_owned()),
            started_at: datetime!(2026-06-28 12:00 UTC),
            ended_at: datetime!(2026-06-28 12:10 UTC),
            start_source: "test".to_owned(),
            end_source: "test".to_owned(),
            end_uncertain: false,
            start_note: None,
            end_note: None,
            battery_observations: vec![BatteryObservation {
                observed_at: datetime!(2026-06-28 12:05 UTC),
                percentage: 60,
                source: BatterySource::BluezPropertiesChanged,
            }],
        };

        let actives = [active];
        let spans = [span];
        let observations = collect_battery_observations(&actives, &spans);

        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0].observation.percentage, 60);
        assert_eq!(observations[1].observation.percentage, 55);
    }
}
