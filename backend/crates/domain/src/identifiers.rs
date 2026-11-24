//! Strongly typed identifiers shared by domain entities.

use std::fmt;

/// A stable identifier for a shopping list.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ListId(String);

/// The authenticated identity of a user (Cognito `sub` in production).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct UserId(String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentifierError {
    Empty,
    TooLong,
}

impl ListId {
    pub fn try_new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        validate_identifier(value.into()).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl UserId {
    pub fn try_new(value: impl Into<String>) -> Result<Self, IdentifierError> {
        validate_identifier(value.into()).map(Self)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn validate_identifier(value: String) -> Result<String, IdentifierError> {
    if value.trim().is_empty() {
        return Err(IdentifierError::Empty);
    }
    if value.chars().count() > 128 {
        return Err(IdentifierError::TooLong);
    }
    Ok(value)
}

impl fmt::Display for ListId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::{IdentifierError, ListId, UserId};

    #[test]
    fn identifiers_reject_empty_values() {
        assert_eq!(ListId::try_new("  "), Err(IdentifierError::Empty));
        assert_eq!(UserId::try_new(""), Err(IdentifierError::Empty));
    }

    #[test]
    fn identifiers_reject_values_longer_than_128_characters() {
        let too_long = "x".repeat(129);
        assert_eq!(ListId::try_new(too_long), Err(IdentifierError::TooLong));
    }

    #[test]
    fn identifiers_preserve_the_original_value() {
        let id = UserId::try_new("cognito-sub-123").expect("valid user id");
        assert_eq!(id.as_str(), "cognito-sub-123");
    }
}
