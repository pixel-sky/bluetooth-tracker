use crate::{
    address::BluetoothAddress,
    display::format_timestamp,
    paths::TrackerPaths,
    storage::{BatteryOutcome, record_manual_battery},
};
use anyhow::Result;
use time::OffsetDateTime;

pub fn set(paths: &TrackerPaths, address: Option<&BluetoothAddress>, percentage: u8) -> Result<()> {
    let observed_at = OffsetDateTime::now_utc();
    let outcome = record_manual_battery(paths, address, percentage, observed_at)?;
    let (target, selected_address) = match outcome {
        BatteryOutcome::ActiveSpan(address) => ("active span", address),
        BatteryOutcome::LatestSpan(address) => ("latest completed span", address),
    };
    println!(
        "Recorded {percentage}% for {selected_address} at {} on the {target}",
        format_timestamp(observed_at)
    );
    Ok(())
}
