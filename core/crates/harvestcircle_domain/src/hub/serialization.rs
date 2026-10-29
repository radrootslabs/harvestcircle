//! One-way archival encoding of validated public values, never command decoding.
use super::{HubId, reference::HubRecordId};
use crate::{PublicKey, UnixTimestamp};
use serde::{Serialize, Serializer};

impl Serialize for HubId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(
            &self
                .as_bytes()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        )
    }
}
impl Serialize for HubRecordId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}
impl Serialize for PublicKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}
impl Serialize for UnixTimestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(self.as_seconds())
    }
}
