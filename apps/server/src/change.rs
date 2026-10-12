//! [`Change`]: one field of a PATCH body that can be left out, set to
//! `null`, or set to a value.

use serde::{Deserialize, Deserializer};

/// A field of a PATCH body. Use with `#[serde(default)]` and
/// `#[schema(value_type = Option<T>)]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Change<T> {
    /// Left out: keep what is there.
    #[default]
    Keep,
    /// `null`: clear it.
    Clear,
    /// A value: replace it.
    Set(T),
}

impl<T> Change<T> {
    /// Whether the field was given at all.
    #[must_use]
    pub const fn is_given(&self) -> bool {
        !matches!(self, Self::Keep)
    }

    /// The value to store if [`is_given`](Self::is_given), as an option.
    #[must_use]
    pub fn into_value(self) -> Option<T> {
        match self {
            Self::Set(value) => Some(value),
            Self::Keep | Self::Clear => None,
        }
    }

    /// Maps the value, if there is one.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Change<U> {
        match self {
            Self::Keep => Change::Keep,
            Self::Clear => Change::Clear,
            Self::Set(value) => Change::Set(f(value)),
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Change<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<T>::deserialize(deserializer).map(|value| value.map_or(Self::Clear, Self::Set))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Body {
        #[serde(default)]
        field: Change<u32>,
    }

    #[test]
    fn left_out_null_and_value_are_three_changes() {
        let body: Body = serde_json::from_str("{}").unwrap();
        assert_eq!(body.field, Change::Keep);
        assert!(!body.field.is_given());
        let body: Body = serde_json::from_str(r#"{"field": null}"#).unwrap();
        assert_eq!(body.field, Change::Clear);
        assert!(body.field.is_given());
        assert_eq!(body.field.into_value(), None);
        let body: Body = serde_json::from_str(r#"{"field": 7}"#).unwrap();
        assert_eq!(
            body.field.clone().map(|n| n.saturating_add(1)),
            Change::Set(8)
        );
        assert_eq!(body.field.into_value(), Some(7));
    }
}
