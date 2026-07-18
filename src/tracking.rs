use crate::{
    address::BluetoothAddress,
    bluez::{self, DEVICE_INTERFACE, DeviceInfo},
    display::{format_duration, format_timestamp},
    paths::TrackerPaths,
    storage::{
        ActiveState, ConnectOutcome, DisconnectOutcome, SpanRecord, mark_connected,
        mark_disconnected, read_jsonl,
    },
};
use ::time::OffsetDateTime;
use anyhow::{Context, Result};
use futures_util::{
    StreamExt,
    stream::{BoxStream, SelectAll},
};
use std::collections::BTreeSet;
use tokio::time::{self, Duration, MissedTickBehavior};
use zbus::{
    Proxy,
    fdo::{PropertiesChanged, PropertiesProxy},
};

#[derive(Default)]
struct WatchState {
    devices: Vec<DeviceInfo>,
    changes: SelectAll<BoxStream<'static, (BluetoothAddress, PropertiesChanged)>>,
    subscribed_addresses: BTreeSet<BluetoothAddress>,
}

struct Observation<'a> {
    trigger: &'a str,
    observation: &'a str,
    end_uncertain: bool,
}

impl<'a> Observation<'a> {
    fn source(&self) -> String {
        format!("{}: {}", self.trigger, self.observation)
    }
}

fn device_label(device: &DeviceInfo) -> String {
    match device.name.as_deref().filter(|name| !name.is_empty()) {
        Some(name) => format!("{name} ({})", device.address),
        None => device.address.to_string(),
    }
}

fn apply_observed_state(
    paths: &TrackerPaths,
    device: &DeviceInfo,
    observed_at: OffsetDateTime,
    observation: Observation<'_>,
) -> Result<()> {
    let source = observation.source();

    if device.connected {
        match mark_connected(paths, device, observed_at, &source)? {
            ConnectOutcome::Started => println!(
                "Connected {} via {} at {}",
                device_label(device),
                source,
                format_timestamp(observed_at)
            ),
            ConnectOutcome::AlreadyActive => {}
        }
    } else {
        match mark_disconnected(
            paths,
            device,
            observed_at,
            &source,
            observation.end_uncertain,
        )? {
            DisconnectOutcome::Closed(span) => println!(
                "Disconnected {} via {} at {} after {}{}",
                device_label(device),
                source,
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

impl WatchState {
    fn has_subscriptions(&self) -> bool {
        !self.changes.is_empty()
    }

    async fn resync(
        &mut self,
        connection: &zbus::Connection,
        paths: &TrackerPaths,
        addresses: impl AsRef<[BluetoothAddress]>,
        trigger: &str,
    ) -> Result<()> {
        let visible_devices = match bluez::list_devices(connection).await {
            Ok(devices) => devices,
            Err(err) => {
                eprintln!("Unable to query Bluetooth devices; will retry later");
                eprintln!("{err:#}");
                return Ok(());
            }
        };

        for address in addresses.as_ref() {
            let device = match visible_devices
                .iter()
                .find(|device| device.address == *address)
            {
                Some(device) => device,
                None => {
                    eprintln!(
                        "Bluetooth device {address} is not currently visible; will retry later"
                    );
                    let missing_device = DeviceInfo {
                        path: String::new(),
                        address: address.clone(),
                        name: None,
                        connected: false,
                    };
                    apply_observed_state(
                        paths,
                        &missing_device,
                        OffsetDateTime::now_utc(),
                        Observation {
                            trigger,
                            observation: "device missing",
                            end_uncertain: true,
                        },
                    )?;
                    if let Some(device) = self
                        .devices
                        .iter_mut()
                        .find(|device| device.address == *address)
                    {
                        device.connected = false;
                    }
                    continue;
                }
            };

            {
                let observation = if device.connected {
                    "device reported connected"
                } else {
                    "device reported disconnected"
                };
                apply_observed_state(
                    paths,
                    device,
                    OffsetDateTime::now_utc(),
                    Observation {
                        trigger,
                        observation,
                        end_uncertain: !device.connected,
                    },
                )
                .with_context(|| {
                    format!(
                        "failed to sync state for {} ({})",
                        device.address, device.path
                    )
                })?;
            }

            self.subscribe(connection, device).await?;

            match self
                .devices
                .iter_mut()
                .find(|tracked| tracked.address == *address)
            {
                Some(tracked) => *tracked = device.clone(),
                None => self.devices.push(device.clone()),
            }
        }

        Ok(())
    }

    async fn subscribe(
        &mut self,
        connection: &zbus::Connection,
        device: &DeviceInfo,
    ) -> Result<()> {
        if self.subscribed_addresses.contains(&device.address) {
            return Ok(());
        }

        let report_subscription_error = |err: &zbus::Error| {
            eprintln!(
                "Failed to subscribe to {} changes; periodic resync will still run",
                device_label(device)
            );
            eprintln!("{err:#}");
        };

        let properties = match PropertiesProxy::builder(connection)
            .destination("org.bluez")?
            .path(device.path.as_str())?
            .build()
            .await
        {
            Ok(properties) => properties,
            Err(err) => {
                report_subscription_error(&err);
                return Ok(());
            }
        };

        match properties.receive_properties_changed().await {
            Ok(stream) => {
                let address = device.address.clone();
                self.changes
                    .push(stream.map(move |signal| (address.clone(), signal)).boxed());
                self.subscribed_addresses.insert(device.address.clone());
            }
            Err(err) => report_subscription_error(&err),
        }

        Ok(())
    }
}

fn unique_addresses(addresses: Vec<BluetoothAddress>) -> Vec<BluetoothAddress> {
    addresses
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub async fn watch(paths: TrackerPaths, addresses: Vec<BluetoothAddress>) -> Result<()> {
    let addresses = unique_addresses(addresses);
    let connection = bluez::system_connection().await?;
    let mut state = WatchState::default();
    state
        .resync(&connection, &paths, &addresses, "startup resync")
        .await?;

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

    {
        let tracking_message = if state.devices.is_empty() {
            format!(
                "Tracking {} configured device{}; none currently visible",
                addresses.len(),
                if addresses.len() == 1 { "" } else { "s" }
            )
        } else {
            format!(
                "Tracking {}",
                state
                    .devices
                    .iter()
                    .map(device_label)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        println!("{tracking_message}");
    }

    let mut changes_enabled = state.has_subscriptions();

    let mut resync = time::interval(Duration::from_secs(60));
    resync.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut sleeping = false;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!("Stopping tracker");
                return Ok(());
            }
            _ = resync.tick(), if !sleeping => {
                state
                    .resync(&connection, &paths, &addresses, "periodic resync")
                    .await?;
                changes_enabled = state.has_subscriptions();
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
                        let observed_at = OffsetDateTime::now_utc();
                        for device in &mut state.devices {
                            device.connected = false;
                            apply_observed_state(
                                &paths,
                                device,
                                observed_at,
                                Observation {
                                    trigger: "system sleep signal",
                                    observation: "system entering sleep",
                                    end_uncertain: false,
                                },
                            )?;
                        }
                    }
                } else {
                    sleeping = false;
                    state
                        .resync(&connection, &paths, &addresses, "wake resync")
                        .await?;
                    changes_enabled = state.has_subscriptions();
                }
            }
            signal = state.changes.next(), if changes_enabled => {
                let Some((address, signal)) = signal else {
                    changes_enabled = false;
                    state.subscribed_addresses.clear();
                    eprintln!("BlueZ PropertiesChanged streams ended; continuing with periodic resync");
                    continue;
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

                let Ok(connected) = bool::try_from(value) else {
                    continue;
                };

                let Some(device) = state
                    .devices
                    .iter_mut()
                    .find(|device| device.address == address)
                else {
                    continue;
                };

                device.connected = connected;
                let observation = if connected {
                    "device reported connected"
                } else {
                    "device reported disconnected"
                };
                apply_observed_state(
                    &paths,
                    device,
                    OffsetDateTime::now_utc(),
                    Observation {
                        trigger: "BlueZ change signal",
                        observation,
                        end_uncertain: false,
                    },
                )?;
            }
        }
    }
}

fn known_device_addresses(paths: &TrackerPaths) -> Result<Vec<BluetoothAddress>> {
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

pub async fn status(paths: TrackerPaths, addresses: Vec<BluetoothAddress>) -> Result<()> {
    let addresses = if addresses.is_empty() {
        known_device_addresses(&paths)?
    } else {
        unique_addresses(addresses)
    };

    if addresses.is_empty() {
        println!("No tracked devices");
        return Ok(());
    }

    let connection = bluez::system_connection().await?;
    let devices = bluez::list_devices(&connection).await?;
    let actives = read_jsonl::<ActiveState>(paths.actives_path())?;
    let spans = read_jsonl::<SpanRecord>(paths.spans_path())?;

    for (index, address) in addresses.iter().enumerate() {
        if index > 0 {
            println!();
        }

        let device = devices.iter().find(|device| device.address == *address);
        let active = actives
            .iter()
            .find(|active| active.device_address == *address);
        let name = device
            .as_ref()
            .and_then(|device| device.name.as_deref())
            .or_else(|| active.and_then(|active| active.device_name.as_deref()))
            .or_else(|| {
                spans
                    .iter()
                    .rev()
                    .find(|span| span.device_address == *address)
                    .and_then(|span| span.device_name.as_deref())
            })
            .unwrap_or("");

        println!("Address: {}", address);
        println!("Name: {}", name);
        println!(
            "Connected: {}",
            device
                .as_ref()
                .map(|device| if device.connected { "yes" } else { "no" })
                .unwrap_or("unknown")
        );

        if let Some(state) = active {
            let elapsed = (OffsetDateTime::now_utc() - state.started_at).whole_seconds();
            println!("Active span: yes");
            println!("Started: {}", format_timestamp(state.started_at));
            println!("Elapsed: {}", format_duration(elapsed));
            if let Some(note) = state.start_note.as_deref() {
                println!("Start note: {note}");
            }
            if let Some(note) = state.end_note.as_deref() {
                println!("End note: {note}");
            }
        } else {
            println!("Active span: no");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{ActiveState, SpanRecord, mark_connected, read_jsonl};
    use ::time::macros::datetime;
    use tempfile::TempDir;

    fn paths(temp: &TempDir) -> TrackerPaths {
        TrackerPaths::new(temp.path())
    }

    fn connected_device() -> DeviceInfo {
        DeviceInfo {
            path: "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF".to_owned(),
            address: BluetoothAddress::new_unchecked("AA:BB:CC:DD:EE:FF"),
            name: Some("Keychron K3".to_owned()),
            connected: true,
        }
    }

    #[test]
    fn connected_observation_starts_span_with_source() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = connected_device();

        apply_observed_state(
            &paths,
            &device,
            datetime!(2026-07-01 12:00 UTC),
            Observation {
                trigger: "startup resync",
                observation: "device reported connected",
                end_uncertain: false,
            },
        )?;

        let actives = read_jsonl::<ActiveState>(paths.actives_path())?;
        assert_eq!(actives.len(), 1);
        assert_eq!(
            actives[0].start_source,
            "startup resync: device reported connected"
        );
        Ok(())
    }

    #[test]
    fn system_sleep_closes_active_span_as_certain() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let mut device = connected_device();

        mark_connected(
            &paths,
            &device,
            datetime!(2026-07-01 12:00 UTC),
            "test-connect",
        )?;
        device.connected = false;
        apply_observed_state(
            &paths,
            &device,
            datetime!(2026-07-01 12:30 UTC),
            Observation {
                trigger: "system sleep signal",
                observation: "system entering sleep",
                end_uncertain: false,
            },
        )?;

        let spans = read_jsonl::<SpanRecord>(paths.spans_path())?;
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].duration_seconds, 1800);
        assert_eq!(
            spans[0].end_source,
            "system sleep signal: system entering sleep"
        );
        assert!(!spans[0].end_uncertain);
        Ok(())
    }

    #[test]
    fn missing_device_closes_active_span_as_uncertain_and_keeps_saved_name() -> Result<()> {
        let temp = TempDir::new()?;
        let paths = paths(&temp);
        let device = connected_device();

        mark_connected(
            &paths,
            &device,
            datetime!(2026-07-01 12:00 UTC),
            "test-connect",
        )?;
        let missing_device = DeviceInfo {
            path: String::new(),
            address: device.address.clone(),
            name: None,
            connected: false,
        };
        apply_observed_state(
            &paths,
            &missing_device,
            datetime!(2026-07-01 12:30 UTC),
            Observation {
                trigger: "startup resync",
                observation: "device missing",
                end_uncertain: true,
            },
        )?;

        let spans = read_jsonl::<SpanRecord>(paths.spans_path())?;
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].duration_seconds, 1800);
        assert_eq!(spans[0].end_source, "startup resync: device missing");
        assert!(spans[0].end_uncertain);
        assert_eq!(spans[0].device_name.as_deref(), Some("Keychron K3"));
        assert!(read_jsonl::<ActiveState>(paths.actives_path())?.is_empty());
        Ok(())
    }
}
