use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{error::Error, fmt, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BluetoothAddress(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddressParseError;

fn normalize_address(address: impl AsRef<str>) -> String {
    address
        .as_ref()
        .trim()
        .replace(['-', '_'], ":")
        .split(':')
        .map(|part| part.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join(":")
}

fn is_valid_address(address: impl AsRef<str>) -> bool {
    let parts = address.as_ref().split(':').collect::<Vec<_>>();
    parts.len() == 6
        && parts
            .iter()
            .all(|part| part.len() == 2 && part.chars().all(|ch| ch.is_ascii_hexdigit()))
}

impl BluetoothAddress {
    pub fn new_unchecked(address: impl AsRef<str>) -> Self {
        Self(normalize_address(address.as_ref()))
    }

    fn parse(address: impl AsRef<str>) -> Result<Self, AddressParseError> {
        let address = normalize_address(address.as_ref());
        if is_valid_address(&address) {
            Ok(Self(address))
        } else {
            Err(AddressParseError)
        }
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
        Self::parse(value)
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
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_unchecked_normalizes_common_separators() {
        assert_eq!(
            BluetoothAddress::new_unchecked("aa:bb:cc:dd:ee:ff").as_str(),
            "AA:BB:CC:DD:EE:FF"
        );
        assert_eq!(
            BluetoothAddress::new_unchecked("aa-bb-cc-dd-ee-ff").as_str(),
            "AA:BB:CC:DD:EE:FF"
        );
        assert_eq!(
            BluetoothAddress::new_unchecked("aa_bb_cc_dd_ee_ff").as_str(),
            "AA:BB:CC:DD:EE:FF"
        );
    }

    #[test]
    fn new_unchecked_does_not_validate() {
        assert_eq!(
            BluetoothAddress::new_unchecked("not-an-address").as_str(),
            "NOT:AN:ADDRESS"
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

    #[test]
    fn rejects_invalid_addresses_when_parsing_user_input() {
        assert!("not-an-address".parse::<BluetoothAddress>().is_err());
        assert!("AA:BB:CC:DD:EE".parse::<BluetoothAddress>().is_err());
        assert!("AA:BB:CC:DD:EE:GG".parse::<BluetoothAddress>().is_err());
        assert!("AA::BB:CC:DD:EE:FF".parse::<BluetoothAddress>().is_err());
        assert!("AA--BB-CC-DD-EE-FF".parse::<BluetoothAddress>().is_err());
    }
}
