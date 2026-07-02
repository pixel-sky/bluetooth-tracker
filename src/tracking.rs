use crate::{
    address::BluetoothAddress,
    bluez::{self, DeviceInfo, DEVICE_INTERFACE},
    display::{format_duration, format_timestamp},
    paths::TrackerPaths,
    storage::{load_active, mark_connected, mark_disconnected, ConnectOutcome, DisconnectOutcome},
};
use ::time::OffsetDateTime;
use anyhow::{Context, Result};
use futures_util::StreamExt;
use tokio::time::{self, Duration, MissedTickBehavior};
use zbus::{fdo::PropertiesProxy, Proxy};

pub async fn watch(paths: TrackerPaths, address: BluetoothAddress) -> Result<()> {
    let connection = bluez::system_connection().await?;
    let mut device = bluez::find_device(&connection, &address).await?;

    sync_current_state(&paths, &device, "startup").with_context(|| {
        format!(
            "failed to sync initial state for {} ({})",
            device.address, device.path
        )
    })?;

    let properties = PropertiesProxy::builder(&connection)
        .destination("org.bluez")?
        .path(device.path.as_str())?
        .build()
        .await?;
    let mut changes = properties.receive_properties_changed().await?;
    let login1 = Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await
    .context("failed to create login1 D-Bus proxy")?;
    let mut sleep_events = login1
        .receive_signal("PrepareForSleep")
        .await
        .context("failed to subscribe to system sleep signals")?;
    let mut resync = time::interval(Duration::from_secs(60));
    resync.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut sleeping = false;

    println!(
        "Tracking {} ({})",
        device.address,
        device.name.as_deref().unwrap_or("unnamed device")
    );

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("Stopping tracker");
                return Ok(());
            }
            _ = resync.tick(), if !sleeping => {
                device = bluez::find_device(&connection, &address).await?;
                sync_current_state(&paths, &device, "periodic-resync")?;
            }
            signal = sleep_events.next() => {
                let Some(signal) = signal else {
                    anyhow::bail!("login1 PrepareForSleep stream ended");
                };
                let entering_sleep = signal
                    .body()
                    .deserialize::<bool>()
                    .context("failed to parse login1 PrepareForSleep signal")?;

                if entering_sleep {
                    if !sleeping {
                        sleeping = true;
                        handle_sleep_start(&paths, &device)?;
                    }
                } else {
                    sleeping = false;
                    device = bluez::find_device(&connection, &address).await?;
                    sync_current_state(&paths, &device, "system-wake")?;
                }
            }
            signal = changes.next() => {
                let Some(signal) = signal else {
                    anyhow::bail!("BlueZ PropertiesChanged stream ended");
                };

                if sleeping {
                    continue;
                }

                let args = signal.args()?;
                if args.interface_name() != DEVICE_INTERFACE {
                    continue;
                }

                let Some(value) = args.changed_properties().get("Connected") else {
                    continue;
                };

                let Some(connected) = bool::try_from(value).ok() else {
                    continue;
                };

                device.connected = connected;
                apply_observed_state(
                    &paths,
                    &device,
                    OffsetDateTime::now_utc(),
                    "dbus-properties-changed",
                    false,
                )?;
            }
        }
    }
}

fn handle_sleep_start(paths: &TrackerPaths, device: &DeviceInfo) -> Result<()> {
    let observed_at = OffsetDateTime::now_utc();
    handle_sleep_start_at(paths, device, observed_at)
}

fn handle_sleep_start_at(
    paths: &TrackerPaths,
    device: &DeviceInfo,
    observed_at: OffsetDateTime,
) -> Result<()> {
    match mark_disconnected(
        paths,
        &device.address,
        device.name.as_deref(),
        observed_at,
        "system-sleep-start",
        false,
    )? {
        DisconnectOutcome::Closed(span) => println!(
            "System sleep at {}; closed span after {}",
            format_timestamp(span.ended_at),
            format_duration(span.duration_seconds),
        ),
        DisconnectOutcome::NoActiveSpan => println!(
            "System sleep at {}; no active span",
            format_timestamp(observed_at)
        ),
    }

    Ok(())
}

pub async fn status(paths: TrackerPaths, address: BluetoothAddress) -> Result<()> {
    let connection = bluez::system_connection().await?;
    let device = bluez::find_device(&connection, &address).await?;

    println!("Address: {}", device.address);
    println!("Name: {}", device.name.as_deref().unwrap_or(""));
    println!("Connected: {}", if device.connected { "yes" } else { "no" });

    match load_active(&paths.state_path)? {
        Some(active) if active.device_address == address => {
            let elapsed = (OffsetDateTime::now_utc() - active.started_at).whole_seconds();
            println!("Active span: yes");
            println!("Started: {}", format_timestamp(active.started_at));
            println!("Elapsed: {}", format_duration(elapsed));
            if let Some(note) = active.start_note.as_deref() {
                println!("Start note: {note}");
            }
            if let Some(note) = active.end_note.as_deref() {
                println!("End note: {note}");
            }
        }
        Some(active) => {
            println!("Active span: no");
            println!(
                "Note: active state exists for another device ({})",
                active.device_address
            );
        }
        None => println!("Active span: no"),
    }

    Ok(())
}

fn sync_current_state(
    paths: &TrackerPaths,
    device: &DeviceInfo,
    source_prefix: &str,
) -> Result<()> {
    let source = if device.connected {
        format!("{source_prefix}-connected")
    } else {
        format!("{source_prefix}-disconnected")
    };

    apply_observed_state(
        paths,
        device,
        OffsetDateTime::now_utc(),
        &source,
        !device.connected,
    )
}

fn apply_observed_state(
    paths: &TrackerPaths,
    device: &DeviceInfo,
    observed_at: OffsetDateTime,
    source: &str,
    end_uncertain: bool,
) -> Result<()> {
    if device.connected {
        match mark_connected(
            paths,
            &device.address,
            device.name.as_deref(),
            observed_at,
            source,
        )? {
            ConnectOutcome::Started => println!("Connected at {}", format_timestamp(observed_at)),
            ConnectOutcome::AlreadyActive => {}
        }
    } else {
        match mark_disconnected(
            paths,
            &device.address,
            device.name.as_deref(),
            observed_at,
            source,
            end_uncertain,
        )? {
            DisconnectOutcome::Closed(span) => println!(
                "Disconnected at {} after {}{}",
                format_timestamp(span.ended_at),
                format_duration(span.duration_seconds),
                if span.end_uncertain {
                    " (uncertain)"
                } else {
                    ""
                }
            ),
            DisconnectOutcome::NoActiveSpan => {}
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{load_spans, mark_connected};
    use ::time::macros::datetime;
    use tempfile::TempDir;

    fn paths(temp: &TempDir) -> TrackerPaths {
        TrackerPaths {
            log_path: temp.path().join("spans.jsonl"),
            state_path: temp.path().join("active.json"),
        }
    }

    #[test]
    fn system_sleep_closes_active_span_as_certain() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = DeviceInfo {
            path: "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF".to_owned(),
            address: BluetoothAddress::new("AA:BB:CC:DD:EE:FF"),
            name: Some("Keychron K3".to_owned()),
            connected: true,
        };

        mark_connected(
            &paths,
            &device.address,
            device.name.as_deref(),
            datetime!(2026-07-01 12:00 UTC),
            "test-connect",
        )?;
        handle_sleep_start_at(&paths, &device, datetime!(2026-07-01 12:30 UTC))?;

        let spans = load_spans(&paths.log_path)?;
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].duration_seconds, 1800);
        assert_eq!(spans[0].end_source, "system-sleep-start");
        assert!(!spans[0].end_uncertain);
        Ok(())
    }
}
