//! Time values shared by account and profile records.

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct UnixTimestamp(i64);

impl UnixTimestamp {
    pub const UNIX_EPOCH: Self = Self(0);

    #[must_use]
    pub const fn from_seconds(seconds: i64) -> Option<Self> {
        if seconds < 0 {
            None
        } else {
            Some(Self(seconds))
        }
    }

    #[must_use]
    pub const fn as_seconds(self) -> i64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::UnixTimestamp;

    #[test]
    fn timestamp_rejects_negative_seconds() {
        assert_eq!(UnixTimestamp::from_seconds(-1), None);
        assert_eq!(
            UnixTimestamp::from_seconds(0).map(UnixTimestamp::as_seconds),
            Some(0)
        );
    }
}
