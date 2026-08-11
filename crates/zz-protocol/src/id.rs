use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

macro_rules! stable_id {
    ($name:ident, $prefix:literal) => {
        #[derive(
            Clone,
            Copy,
            Debug,
            Default,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
        )]
        #[repr(transparent)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, concat!($prefix, "{}"), self.0)
            }
        }

        impl FromStr for $name {
            type Err = ParseIdError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let raw = value
                    .strip_prefix($prefix)
                    .ok_or(ParseIdError::MissingPrefix($prefix))?;
                let id = raw
                    .parse::<u64>()
                    .map_err(|_| ParseIdError::InvalidNumber(value.to_owned()))?;
                Ok(Self(id))
            }
        }
    };
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseIdError {
    #[error("identifier must start with {0}")]
    MissingPrefix(&'static str),
    #[error("invalid identifier: {0}")]
    InvalidNumber(String),
}

stable_id!(SessionId, "$");
stable_id!(WindowId, "@");
stable_id!(PaneId, "%");
stable_id!(SplitId, "^");
stable_id!(ClientId, "c");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_ids_round_trip() {
        assert_eq!("$42".parse::<SessionId>().unwrap(), SessionId(42));
        assert_eq!(WindowId(7).to_string(), "@7");
        assert_eq!(PaneId(9).to_string(), "%9");
        assert_eq!("^11".parse::<SplitId>().unwrap(), SplitId(11));
        assert!("9".parse::<PaneId>().is_err());
    }
}
