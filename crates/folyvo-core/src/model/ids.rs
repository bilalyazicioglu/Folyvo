use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    #[error("invalid id {got:?}, expected prefix {expected:?} followed by lowercase hex")]
    Invalid { expected: &'static str, got: String },
}

fn is_lower_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

macro_rules! prefixed_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(String);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            pub fn parse(s: &str) -> Result<Self, IdError> {
                match s.strip_prefix($prefix) {
                    Some(rest) if is_lower_hex(rest) => Ok(Self(s.to_owned())),
                    _ => Err(IdError::Invalid { expected: $prefix, got: s.to_owned() }),
                }
            }

            /// Build from a hex suffix. Panics if the suffix is not lowercase hex.
            pub fn from_suffix(suffix: &str) -> Self {
                Self::parse(&format!("{}{}", $prefix, suffix))
                    .expect("suffix must be non-empty lowercase hex")
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

prefixed_id!(ItemId, "itm_");
prefixed_id!(VersionId, "ver_");

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentHash(String);

impl ContentHash {
    pub const PREFIX: &'static str = "b3_";

    pub fn of_bytes(bytes: &[u8]) -> Self {
        Self::from_blake3(blake3::hash(bytes))
    }

    pub fn from_blake3(h: blake3::Hash) -> Self {
        Self(format!("{}{}", Self::PREFIX, h.to_hex()))
    }

    pub fn parse(s: &str) -> Result<Self, IdError> {
        match s.strip_prefix(Self::PREFIX) {
            Some(rest) if rest.len() == 64 && is_lower_hex(rest) => Ok(Self(s.to_owned())),
            _ => Err(IdError::Invalid { expected: Self::PREFIX, got: s.to_owned() }),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// (first two hex chars, remaining 62) for the object-store fan-out directory.
    pub fn shard(&self) -> (&str, &str) {
        let hex = &self.0[Self::PREFIX.len()..];
        hex.split_at(2)
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_id_round_trips_and_keeps_prefix() {
        let id = ItemId::from_suffix("00ff");
        assert_eq!(id.as_str(), "itm_00ff");
        assert_eq!(ItemId::parse("itm_00ff").unwrap(), id);
    }

    #[test]
    fn parse_rejects_wrong_prefix_empty_suffix_and_uppercase() {
        assert!(ItemId::parse("ver_00ff").is_err());
        assert!(ItemId::parse("itm_").is_err());
        assert!(ItemId::parse("itm_00FF").is_err());
        assert!(VersionId::parse("itm_00ff").is_err());
    }

    #[test]
    fn content_hash_is_prefixed_64_lowercase_hex() {
        let h = ContentHash::of_bytes(b"");
        assert!(h.as_str().starts_with("b3_"));
        assert_eq!(h.as_str().len(), 3 + 64);
        assert_eq!(ContentHash::parse(h.as_str()).unwrap(), h);
    }

    #[test]
    fn content_hash_of_empty_input_is_the_known_blake3_value() {
        assert_eq!(
            ContentHash::of_bytes(b"").as_str(),
            "b3_af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
    }

    #[test]
    fn content_hash_shards_into_two_chars_and_rest() {
        let h = ContentHash::of_bytes(b"x");
        let (a, b) = h.shard();
        assert_eq!(a.len(), 2);
        assert_eq!(b.len(), 62);
        assert_eq!(format!("b3_{a}{b}"), h.as_str());
    }

    #[test]
    fn content_hash_rejects_short_input() {
        assert!(ContentHash::parse("b3_abc").is_err());
    }
}
