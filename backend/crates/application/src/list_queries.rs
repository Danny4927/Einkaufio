//! Read use cases: cursor-paginated lists and items (roadmap B3) and version-based sync
//! (roadmap B5).

use einkaufio_domain::{
    AggregateVersion, ListId, ListOperation, ShoppingItem, ShoppingList, UserId,
};

use crate::ports::{
    ListAccessRequest, ListSync, Page, PageCursor, PageRequest, PageSize, RepositoryError,
    ShoppingItemRepository, ShoppingListRepository,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ListQueryError {
    InvalidPrincipal,
    InvalidListId,
    InvalidPageSize,
    InvalidCursor,
    InvalidSinceVersion,
    /// The list does not exist or the principal cannot access it (indistinguishable).
    NotFound,
    Forbidden,
    PersistenceUnavailable,
    RepositoryInvariantViolation,
}

/// Page parameters as received from the transport; `None` page size means the default.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PageParameters {
    pub page_size: Option<u32>,
    pub cursor: Option<String>,
}

fn page_request(parameters: PageParameters) -> Result<PageRequest, ListQueryError> {
    let size = parameters
        .page_size
        .map(PageSize::try_new)
        .transpose()
        .map_err(|_| ListQueryError::InvalidPageSize)?
        .unwrap_or(PageSize::DEFAULT);
    let after = parameters
        .cursor
        .map(PageCursor::try_new)
        .transpose()
        .map_err(|_| ListQueryError::InvalidCursor)?;
    Ok(PageRequest { size, after })
}

fn map_repository_error(error: RepositoryError) -> ListQueryError {
    match error {
        RepositoryError::NotFound => ListQueryError::NotFound,
        RepositoryError::Forbidden => ListQueryError::Forbidden,
        RepositoryError::InvalidCursor => ListQueryError::InvalidCursor,
        RepositoryError::Unavailable => ListQueryError::PersistenceUnavailable,
        RepositoryError::AlreadyExists
        | RepositoryError::Conflict
        | RepositoryError::ItemLimitReached
        | RepositoryError::VersionExhausted => ListQueryError::RepositoryInvariantViolation,
    }
}

fn principal(value: String) -> Result<UserId, ListQueryError> {
    UserId::try_new(value).map_err(|_| ListQueryError::InvalidPrincipal)
}

fn list_id(value: String) -> Result<ListId, ListQueryError> {
    ListId::try_new(value).map_err(|_| ListQueryError::InvalidListId)
}

fn check_page_bound<T>(page: Page<T>, request: &PageRequest) -> Result<Page<T>, ListQueryError> {
    if page.items.len() > request.size.as_usize() {
        return Err(ListQueryError::RepositoryInvariantViolation);
    }
    Ok(page)
}

pub struct PageAccessibleLists<'a, R> {
    repository: &'a R,
}

impl<'a, R: ShoppingListRepository> PageAccessibleLists<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    /// `principal_id` comes from verified authentication context only.
    pub async fn execute(
        &self,
        principal_id: String,
        parameters: PageParameters,
    ) -> Result<Page<ShoppingList>, ListQueryError> {
        let principal = principal(principal_id)?;
        let request = page_request(parameters)?;
        let page = self
            .repository
            .list_accessible_page(&principal, &request)
            .await
            .map_err(map_repository_error)?;
        check_page_bound(page, &request)
    }
}

pub struct PageShoppingItems<'a, R> {
    repository: &'a R,
}

impl<'a, R: ShoppingItemRepository> PageShoppingItems<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    pub async fn execute(
        &self,
        principal_id: String,
        list: String,
        parameters: PageParameters,
    ) -> Result<Page<ShoppingItem>, ListQueryError> {
        let access =
            ListAccessRequest::for_operation(principal(principal_id)?, ListOperation::ReadItems);
        let list_id = list_id(list)?;
        let request = page_request(parameters)?;
        let page = self
            .repository
            .list_items_page(&access, &list_id, &request)
            .await
            .map_err(map_repository_error)?;
        check_page_bound(page, &request)
    }
}

pub struct SyncShoppingList<'a, R> {
    repository: &'a R,
}

impl<'a, R: ShoppingItemRepository> SyncShoppingList<'a, R> {
    pub fn new(repository: &'a R) -> Self {
        Self { repository }
    }

    /// Returns changes after `since_version`, or a snapshot when it is `None`.
    pub async fn execute(
        &self,
        principal_id: String,
        list: String,
        since_version: Option<u64>,
    ) -> Result<ListSync, ListQueryError> {
        let access =
            ListAccessRequest::for_operation(principal(principal_id)?, ListOperation::ReadItems);
        let list_id = list_id(list)?;
        let since = since_version
            .map(AggregateVersion::try_new)
            .transpose()
            .map_err(|_| ListQueryError::InvalidSinceVersion)?;
        let sync = self
            .repository
            .sync_list(&access, &list_id, since)
            .await
            .map_err(map_repository_error)?;
        // Changes must belong to the requested list and baseline; anything else is an
        // adapter defect that must not reach clients as a silently wrong delta.
        let (list, baseline) = match &sync {
            ListSync::Snapshot { list, .. } => (list, None),
            ListSync::Changes { list, since, .. } => (list, Some(*since)),
        };
        if list.id() != &list_id || (baseline.is_some() && baseline != since) {
            return Err(ListQueryError::RepositoryInvariantViolation);
        }
        Ok(sync)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use einkaufio_domain::{ItemId, ItemName, ListName, ListPermission};

    use super::*;
    use crate::test_support::BlockOn;

    fn list() -> ShoppingList {
        ShoppingList::new_personal(
            ListId::try_new("list-1").unwrap(),
            UserId::try_new("owner").unwrap(),
            ListName::try_new("Wochenmarkt").unwrap(),
        )
    }

    fn item(id: &str) -> ShoppingItem {
        ShoppingItem::new(
            ItemId::try_new(id).unwrap(),
            ItemName::try_new("Milch").unwrap(),
            None,
            None,
            None,
            0,
            UserId::try_new("owner").unwrap(),
        )
    }

    #[derive(Default)]
    struct Fake {
        failure: Option<RepositoryError>,
        oversized: bool,
        sync: Option<ListSync>,
        list_calls: Mutex<Vec<(UserId, PageRequest)>>,
        item_calls: Mutex<Vec<(ListAccessRequest, ListId, PageRequest)>>,
        sync_calls: Mutex<Vec<(ListAccessRequest, Option<AggregateVersion>)>>,
    }

    impl ShoppingListRepository for Fake {
        async fn create_personal(&self, _: ShoppingList) -> Result<(), RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
        async fn list_accessible_page(
            &self,
            principal: &UserId,
            page: &PageRequest,
        ) -> Result<Page<ShoppingList>, RepositoryError> {
            self.list_calls
                .lock()
                .unwrap()
                .push((principal.clone(), page.clone()));
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            let count = if self.oversized {
                page.size.as_usize() + 1
            } else {
                1
            };
            Ok(Page {
                items: vec![list(); count],
                next: Some(PageCursor::try_new("next").unwrap()),
            })
        }
    }

    impl ShoppingItemRepository for Fake {
        async fn add_item(
            &self,
            _: &ListAccessRequest,
            _: &ListId,
            _: AggregateVersion,
            _: ShoppingItem,
        ) -> Result<AggregateVersion, RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
        async fn set_item_checked(
            &self,
            _: &ListAccessRequest,
            _: &ListId,
            _: &ItemId,
            _: u64,
            _: bool,
        ) -> Result<(ShoppingItem, AggregateVersion), RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
        async fn delete_item(
            &self,
            _: &ListAccessRequest,
            _: &ListId,
            _: &ItemId,
            _: u64,
        ) -> Result<AggregateVersion, RepositoryError> {
            Err(RepositoryError::Unavailable)
        }
        async fn list_items_page(
            &self,
            access: &ListAccessRequest,
            list_id: &ListId,
            page: &PageRequest,
        ) -> Result<Page<ShoppingItem>, RepositoryError> {
            self.item_calls
                .lock()
                .unwrap()
                .push((access.clone(), list_id.clone(), page.clone()));
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            Ok(Page {
                items: vec![item("a")],
                next: None,
            })
        }
        async fn sync_list(
            &self,
            access: &ListAccessRequest,
            _: &ListId,
            since: Option<AggregateVersion>,
        ) -> Result<ListSync, RepositoryError> {
            self.sync_calls
                .lock()
                .unwrap()
                .push((access.clone(), since));
            if let Some(error) = &self.failure {
                return Err(error.clone());
            }
            self.sync.clone().ok_or(RepositoryError::Unavailable)
        }
    }

    #[test]
    fn list_pages_default_and_bound_the_page_size() {
        let fake = Fake::default();
        let use_case = PageAccessibleLists::new(&fake);
        let page = use_case
            .execute("owner".into(), PageParameters::default())
            .block_on()
            .unwrap();
        assert_eq!(page.items.len(), 1);
        let calls = fake.list_calls.lock().unwrap().clone();
        assert_eq!(calls[0].0.as_str(), "owner");
        assert_eq!(calls[0].1.size, PageSize::DEFAULT);
        assert_eq!(calls[0].1.after, None);
        for size in [0, PageSize::MAX + 1] {
            assert_eq!(
                use_case
                    .execute(
                        "owner".into(),
                        PageParameters {
                            page_size: Some(size),
                            cursor: None
                        }
                    )
                    .block_on(),
                Err(ListQueryError::InvalidPageSize)
            );
        }
        for cursor in ["", "with space", &"x".repeat(PageCursor::MAX_LENGTH + 1)] {
            assert_eq!(
                use_case
                    .execute(
                        "owner".into(),
                        PageParameters {
                            page_size: None,
                            cursor: Some(cursor.to_owned())
                        }
                    )
                    .block_on(),
                Err(ListQueryError::InvalidCursor)
            );
        }
        assert_eq!(fake.list_calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn oversized_adapter_pages_are_rejected() {
        let fake = Fake {
            oversized: true,
            ..Fake::default()
        };
        assert_eq!(
            PageAccessibleLists::new(&fake)
                .execute(
                    "owner".into(),
                    PageParameters {
                        page_size: Some(2),
                        cursor: None
                    }
                )
                .block_on(),
            Err(ListQueryError::RepositoryInvariantViolation)
        );
    }

    #[test]
    fn item_pages_require_read_permission_and_pass_the_cursor() {
        let fake = Fake::default();
        PageShoppingItems::new(&fake)
            .execute(
                "member".into(),
                "list-1".into(),
                PageParameters {
                    page_size: Some(10),
                    cursor: Some("abc".into()),
                },
            )
            .block_on()
            .unwrap();
        let calls = fake.item_calls.lock().unwrap();
        assert_eq!(calls[0].0.principal().as_str(), "member");
        assert_eq!(calls[0].0.permission(), ListPermission::Contribute);
        assert_eq!(calls[0].1.as_str(), "list-1");
        assert_eq!(calls[0].2.size.get(), 10);
        assert_eq!(calls[0].2.after.as_ref().unwrap().as_str(), "abc");
    }

    #[test]
    fn query_failures_keep_not_found_and_forbidden_distinct() {
        for (failure, expected) in [
            (RepositoryError::NotFound, ListQueryError::NotFound),
            (RepositoryError::Forbidden, ListQueryError::Forbidden),
            (
                RepositoryError::InvalidCursor,
                ListQueryError::InvalidCursor,
            ),
            (
                RepositoryError::Unavailable,
                ListQueryError::PersistenceUnavailable,
            ),
            (
                RepositoryError::Conflict,
                ListQueryError::RepositoryInvariantViolation,
            ),
        ] {
            let fake = Fake {
                failure: Some(failure),
                ..Fake::default()
            };
            assert_eq!(
                PageShoppingItems::new(&fake)
                    .execute("u".into(), "l".into(), PageParameters::default())
                    .block_on(),
                Err(expected.clone())
            );
            assert_eq!(
                SyncShoppingList::new(&fake)
                    .execute("u".into(), "l".into(), Some(1))
                    .block_on(),
                Err(expected)
            );
        }
    }

    #[test]
    fn sync_validates_since_and_returns_adapter_result() {
        let changes = ListSync::Changes {
            list: list(),
            since: AggregateVersion::try_new(3).unwrap(),
            changed: vec![item("a")],
            removed: vec![ItemId::try_new("b").unwrap()],
        };
        let fake = Fake {
            sync: Some(changes.clone()),
            ..Fake::default()
        };
        let use_case = SyncShoppingList::new(&fake);
        assert_eq!(
            use_case
                .execute("owner".into(), "list-1".into(), Some(0))
                .block_on(),
            Err(ListQueryError::InvalidSinceVersion)
        );
        assert_eq!(
            use_case
                .execute("owner".into(), "list-1".into(), Some(3))
                .block_on(),
            Ok(changes)
        );
        let calls = fake.sync_calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0.permission(), ListPermission::Contribute);
        assert_eq!(calls[0].1, Some(AggregateVersion::try_new(3).unwrap()));
    }

    #[test]
    fn sync_rejects_results_for_another_list_or_baseline() {
        let fake = Fake {
            sync: Some(ListSync::Changes {
                list: list(),
                since: AggregateVersion::try_new(2).unwrap(),
                changed: Vec::new(),
                removed: Vec::new(),
            }),
            ..Fake::default()
        };
        let use_case = SyncShoppingList::new(&fake);
        assert_eq!(
            use_case
                .execute("owner".into(), "list-1".into(), Some(3))
                .block_on(),
            Err(ListQueryError::RepositoryInvariantViolation)
        );
        assert_eq!(
            use_case
                .execute("owner".into(), "other".into(), Some(2))
                .block_on(),
            Err(ListQueryError::RepositoryInvariantViolation)
        );
    }
}
