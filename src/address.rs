use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{error::Error, fmt, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BluetoothAddress(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressParseError;

impl BluetoothAddress {
    pub fn new(address: impl AsRef<str>) -> Self {
        Self(normalize_address(address.as_ref()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for BluetoothAddress {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl fmt::Display for AddressParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("invalid Bluetooth address")
    }
}

impl Error for AddressParseError {}

impl FromStr for BluetoothAddress {
    type Err = AddressParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(value))
    }
}

impl From<&str> for BluetoothAddress {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for BluetoothAddress {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl Serialize for BluetoothAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BluetoothAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer).map(Self::new)
    }
}

fn normalize_address(address: &str) -> String {
    address
        .trim()
        .replace(['-', '_'], ":")
        .split(':')
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_common_separators() {
        assert_eq!(
            BluetoothAddress::new("aa:bb:cc:dd:ee:ff").as_str(),
            "AA:BB:CC:DD:EE:FF"
        );
        assert_eq!(
            BluetoothAddress::new("aa-bb-cc-dd-ee-ff").as_str(),
            "AA:BB:CC:DD:EE:FF"
        );
        assert_eq!(
            BluetoothAddress::new("aa_bb_cc_dd_ee_ff").as_str(),
            "AA:BB:CC:DD:EE:FF"
        );
    }

    #[test]
    fn deserializes_as_normalized_string() {
        let address: BluetoothAddress = serde_json::from_str("\"aa-bb-cc-dd-ee-ff\"").unwrap();
        assert_eq!(address.as_str(), "AA:BB:CC:DD:EE:FF");
        assert_eq!(
            serde_json::to_string(&address).unwrap(),
            "\"AA:BB:CC:DD:EE:FF\""
        );
    }
}
