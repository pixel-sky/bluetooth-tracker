use crate::address::BluetoothAddress;
use std::collections::BTreeSet;

pub(crate) fn unique_addresses(addresses: Vec<BluetoothAddress>) -> Vec<BluetoothAddress> {
    addresses
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
