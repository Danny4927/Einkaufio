//! Access model for shopping lists (ADR 015).
//!
//! A principal holds at most one effective [`ListAccess`] on a list, resolved from persisted
//! ownership or membership records. Every list operation requires a [`ListPermission`]; the
//! mapping from operation to permission is the single authorization rule table
//! ([`ListOperation::required_permission`]).

/// Effective access a principal holds on a list.
///
/// Variants are ordered from weakest to strongest so that a principal holding access through
/// several paths (for example personal ownership and, later, household membership) resolves
/// to the strongest one with `max`.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ListAccess {
    /// May use the list's content: read the list and its items and mutate items.
    ///
    /// Granted by active membership of the owning household (roadmap C2) and, from v1.x,
    /// by explicit list membership.
    Contributor,
    /// May additionally administer the list: rename and delete it.
    ///
    /// Held by the owner of a personal list and (roadmap C2) the owner of the owning
    /// household.
    Administrator,
}

/// Permission an operation requires on its target list.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ListPermission {
    Contribute,
    Administer,
}

/// List operations that require authorization against an existing list.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ListOperation {
    ReadItems,
    EditItems,
    Rename,
    Delete,
}

impl ListAccess {
    /// Whether this access satisfies the required permission.
    pub const fn allows(self, permission: ListPermission) -> bool {
        match (self, permission) {
            (Self::Administrator, _) | (Self::Contributor, ListPermission::Contribute) => true,
            (Self::Contributor, ListPermission::Administer) => false,
        }
    }
}

impl ListOperation {
    /// The authorization rule table for list operations.
    pub const fn required_permission(self) -> ListPermission {
        match self {
            Self::ReadItems | Self::EditItems => ListPermission::Contribute,
            Self::Rename | Self::Delete => ListPermission::Administer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ListAccess, ListOperation, ListPermission};

    #[test]
    fn administrator_satisfies_every_permission() {
        assert!(ListAccess::Administrator.allows(ListPermission::Contribute));
        assert!(ListAccess::Administrator.allows(ListPermission::Administer));
    }

    #[test]
    fn contributor_cannot_administer() {
        assert!(ListAccess::Contributor.allows(ListPermission::Contribute));
        assert!(!ListAccess::Contributor.allows(ListPermission::Administer));
    }

    #[test]
    fn strongest_access_wins_when_combined() {
        assert_eq!(
            Some(ListAccess::Contributor).max(Some(ListAccess::Administrator)),
            Some(ListAccess::Administrator)
        );
        assert_eq!(
            None.max(Some(ListAccess::Contributor)),
            Some(ListAccess::Contributor)
        );
    }

    #[test]
    fn item_operations_require_contribution_and_list_administration_requires_admin() {
        assert_eq!(
            ListOperation::ReadItems.required_permission(),
            ListPermission::Contribute
        );
        assert_eq!(
            ListOperation::EditItems.required_permission(),
            ListPermission::Contribute
        );
        assert_eq!(
            ListOperation::Rename.required_permission(),
            ListPermission::Administer
        );
        assert_eq!(
            ListOperation::Delete.required_permission(),
            ListPermission::Administer
        );
    }
}
