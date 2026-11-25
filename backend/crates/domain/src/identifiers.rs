//! Strongly typed identifiers shared by domain entities.

use std::fmt;

/// A stable identifier for a shopping list.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ListId(String);

/// The authenticated identity of a user (Cognito `sub` in production).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct UserId(String);

/// A stable identifier for a shopping-list item.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ItemId(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentifierError {
    Empty,
    TooLong,
    ContainsControlCharacters,
}

macro_rules! identifier {
    ($type:ident) => {
        impl $type {
            pub fn try_new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(IdentifierError::Empty);
                }
                if value.chars().count() > 128 {
                    return Err(IdentifierError::TooLong);
                }
                if value.chars().any(char::is_control) {
                    return Err(IdentifierError::ContainsControlCharacters);
                }
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }
    };
}

identifier!(ListId);
identifier!(UserId);
identifier!(ItemId);

#[cfg(test)]
mod tests {
    use super::{IdentifierError, ItemId, ListId, UserId};

    #[test]
    fn identifiers_reject_empty_values() {
        assert_eq!(ListId::try_new("  "), Err(IdentifierError::Empty));
        assert_eq!(UserId::try_new(""), Err(IdentifierError::Empty));
        assert_eq!(ItemId::try_new(" "), Err(IdentifierError::Empty));
    }

    #[test]
    fn identifiers_reject_values_longer_than_128_characters() {
        let too_long = "x".repeat(129);
        assert_eq!(ListId::try_new(too_long.clone()), Err(IdentifierError::TooLong));
        assert_eq!(ItemId::try_new(too_long), Err(IdentifierError::TooLong));
    }

    #[test]
    fn identifiers_reject_control_characters() {
        assert_eq!(
            UserId::try_new("user\n1"),
            Err(IdentifierError::ContainsControlCharacters)
        );
    }

    #[test]
    fn identifiers_preserve_the_original_value() {
        let id = UserId::try_new("cognito-sub-123").expect("valid user id");
        assert_eq!(id.as_str(), "cognito-sub-123");
    }
}
