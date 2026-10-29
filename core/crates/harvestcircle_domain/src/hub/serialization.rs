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

impl Serialize for super::reference::SampleHandlingBasis {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Basis {
            id: &'static str,
            version: u32,
            site_timezone: &'static str,
        }
        Basis {
            id: self.id(),
            version: self.version(),
            site_timezone: self.site_timezone(),
        }
        .serialize(serializer)
    }
}
