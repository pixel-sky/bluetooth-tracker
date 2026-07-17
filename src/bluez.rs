use anyhow::{Context, Result};
use std::collections::HashMap;
use zbus::{Connection, fdo::ObjectManagerProxy, names::OwnedInterfaceName, zvariant::OwnedValue};

use crate::address::BluetoothAddress;

const BLUEZ_DESTINATION: &str = "org.bluez";
const OBJECT_MANAGER_PATH: &str = "/";
pub const DEVICE_INTERFACE: &str = "org.bluez.Device1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub path: String,
    pub address: BluetoothAddress,
    pub name: Option<String>,
    pub connected: bool,
}

pub async fn system_connection() -> Result<Connection> {
    Connection::system()
        .await
        .context("failed to connect to the system D-Bus")
}

pub async fn discover() -> Result<()> {
    let connection = system_connection().await?;
    let devices = list_devices(&connection).await?;

    println!("{:<17} {:<9} NAME", "ADDRESS", "CONNECTED");
    for device in devices {
        println!(
            "{:<17} {:<9} {}",
            device.address,
            if device.connected { "yes" } else { "no" },
            device.name.unwrap_or_default()
        );
    }

    Ok(())
}

pub async fn list_devices(connection: &Connection) -> Result<Vec<DeviceInfo>> {
    let manager = ObjectManagerProxy::builder(connection)
        .destination(BLUEZ_DESTINATION)?
        .path(OBJECT_MANAGER_PATH)?
        .build()
        .await?;
    let objects = manager.get_managed_objects().await?;

    let mut devices = objects
        .into_iter()
        .filter_map(|(path, mut interfaces)| {
            let properties =
                interfaces.remove(&OwnedInterfaceName::try_from(DEVICE_INTERFACE).ok()?)?;
            let address = property::<String>(&properties, "Address")?;
            let alias = property::<String>(&properties, "Alias");
            let name = property::<String>(&properties, "Name");
            let connected = property::<bool>(&properties, "Connected").unwrap_or(false);

            Some(DeviceInfo {
                path: path.to_string(),
                address: BluetoothAddress::new_unchecked(address),
                name: alias.or(name),
                connected,
            })
        })
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| left.address.cmp(&right.address));
    Ok(devices)
}

fn property<T>(properties: &HashMap<String, OwnedValue>, name: impl AsRef<str>) -> Option<T>
where
    T: TryFrom<OwnedValue>,
{
    properties
        .get(name.as_ref())
        .and_then(|value| T::try_from(value.clone()).ok())
}
