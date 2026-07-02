use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use zbus::{
    fdo::ObjectManagerProxy,
    names::OwnedInterfaceName,
    zvariant::{OwnedObjectPath, OwnedValue},
    Connection,
};

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

type InterfaceProperties = HashMap<OwnedInterfaceName, HashMap<String, OwnedValue>>;

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
        .filter_map(|(path, interfaces)| device_from_interfaces(path, interfaces))
        .collect::<Vec<_>>();
    devices.sort_by(|left, right| left.address.cmp(&right.address));
    Ok(devices)
}

pub async fn find_device(
    connection: &Connection,
    address: &BluetoothAddress,
) -> Result<DeviceInfo> {
    list_devices(connection)
        .await?
        .into_iter()
        .find(|device| device.address == *address)
        .ok_or_else(|| anyhow!("Bluetooth device {address} was not found in BlueZ"))
}

fn device_from_interfaces(
    path: OwnedObjectPath,
    mut interfaces: InterfaceProperties,
) -> Option<DeviceInfo> {
    let properties = interfaces.remove(&OwnedInterfaceName::try_from(DEVICE_INTERFACE).ok()?)?;
    let address = string_property(&properties, "Address")?;
    let alias = string_property(&properties, "Alias");
    let name = string_property(&properties, "Name");
    let connected = bool_property(&properties, "Connected").unwrap_or(false);

    Some(DeviceInfo {
        path: path.to_string(),
        address: BluetoothAddress::new(address),
        name: alias.or(name),
        connected,
    })
}

pub fn bool_property(properties: &HashMap<String, OwnedValue>, name: &str) -> Option<bool> {
    properties
        .get(name)
        .and_then(|value| bool::try_from(value.clone()).ok())
}

pub fn string_property(properties: &HashMap<String, OwnedValue>, name: &str) -> Option<String> {
    properties
        .get(name)
        .and_then(|value| String::try_from(value.clone()).ok())
}
