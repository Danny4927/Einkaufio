//! Household and invitation ports (roadmap C2–C4, D2; ADR 015, ADR 017, ADR 019).
//!
//! Limits are enforced inside the writing transaction: the member count lives on the
//! household metadata and changes with its version, the per-user household count and the
//! per-household list count are counter records with bound conditions, and invitation
//! creation is serialized by a revision record so that the valid-invitation count read
//! before the write is still exact when the write commits.

use einkaufio_application::{
    HouseholdAccessRequest, HouseholdDetails, HouseholdRepository, HouseholdRepositoryError,
    HouseholdSummary, InvitationRepository,
};
use einkaufio_domain::{
    AggregateVersion, Household, HouseholdId, HouseholdMember, HouseholdName, HouseholdPermission,
    HouseholdRole, Invitation, InvitationId, InvitationTokenHash, ShoppingList, Timestamp, UserId,
};

use super::DynamoDbRepository;
use super::keys::{self, Key};
use super::records::{
    self, ACTIVE, COUNT, CREATED_BY, DELETING, HouseholdRecord, InvitationRef, ListRecord,
    OWNER_ID, REV, REVOKED, STATE, TOKEN_HASH, USED_COUNT, VERSION,
};
use super::store::{
    Action, Check, Commit, Condition, Item, MAX_ATTEMPTS, MAX_TRANSACTION_ACTIONS, StoreError,
    keys_of, number, sort_key, string,
};

type Error = HouseholdRepositoryError;

impl From<StoreError> for HouseholdRepositoryError {
    fn from(_: StoreError) -> Self {
        HouseholdRepositoryError::Unavailable
    }
}

fn version_exhausted<E>(_: E) -> Error {
    Error::VersionExhausted
}

fn limit(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// The household metadata must be unchanged since it was read (every visible change,
/// including every membership change, advances the version) and, for administrative
/// requests, still owned by the principal.
fn meta_condition(record: &HouseholdRecord, request: Option<&HouseholdAccessRequest>) -> Condition {
    let condition = Condition::present()
        .equals(VERSION, number(record.household.version().as_u64()))
        .equals(STATE, string(ACTIVE));
    match request {
        Some(request) if request.permission() == HouseholdPermission::Administer => {
            condition.equals(OWNER_ID, string(request.principal().as_str()))
        }
        _ => condition,
    }
}

/// Re-checks access for writes that do not replace the household metadata (ADR 015).
fn guard(household: &HouseholdId, request: &HouseholdAccessRequest) -> Action {
    match request.permission() {
        HouseholdPermission::Administer => Action::Check {
            key: keys::household_meta(household),
            condition: Condition::present()
                .equals(STATE, string(ACTIVE))
                .equals(OWNER_ID, string(request.principal().as_str())),
        },
        HouseholdPermission::Participate => Action::Check {
            key: keys::member(household, request.principal()),
            condition: Condition::present(),
        },
    }
}

fn put_household(next: &HouseholdRecord, condition: Condition) -> Action {
    Action::Put {
        item: records::encode_household(next),
        condition,
    }
}

/// Membership record, user reference and per-user counter change together.
fn start_membership(household: &HouseholdId, user: &UserId) -> Vec<Action> {
    vec![
        Action::Put {
            item: records::encode_member(household, user),
            condition: Condition::missing(),
        },
        Action::Put {
            item: records::encode_household_ref(user, household),
            condition: Condition::none(),
        },
        Action::Add {
            key: keys::user_household_count(user),
            attribute: COUNT,
            delta: 1,
            condition: Condition::none().and(Check::MissingOrLessThan(
                COUNT,
                number(limit(Household::MAX_HOUSEHOLDS_PER_USER)),
            )),
        },
    ]
}

fn end_membership(household: &HouseholdId, user: &UserId) -> Vec<Action> {
    vec![
        Action::Delete {
            key: keys::member(household, user),
            condition: Condition::present(),
        },
        Action::Delete {
            key: keys::user_household_ref(user, household),
            condition: Condition::none(),
        },
        Action::Add {
            key: keys::user_household_count(user),
            attribute: COUNT,
            delta: -1,
            condition: Condition::none().and(Check::GreaterThan(COUNT, number(0))),
        },
    ]
}

fn invitation_ref(invitation: &Invitation) -> InvitationRef {
    InvitationRef {
        household: invitation.household_id().clone(),
        invitation: invitation.id().clone(),
        hash: *invitation.token_hash(),
    }
}

impl DynamoDbRepository {
    async fn read_household(
        &self,
        household: &HouseholdId,
    ) -> Result<Option<HouseholdRecord>, Error> {
        Ok(self
            .store
            .get(&keys::household_meta(household))
            .await?
            .map(|item| records::decode_household(&item))
            .transpose()?)
    }

    /// Authorizes from persisted membership: no active membership (or no active household)
    /// → `NotFound`; membership too weak → `Forbidden`. `extra` records are read in the same
    /// snapshot and returned in order.
    async fn authorize_household(
        &self,
        request: &HouseholdAccessRequest,
        household: &HouseholdId,
        extra: &[Key],
    ) -> Result<(HouseholdRecord, Vec<Option<Item>>), Error> {
        let mut keys = vec![
            keys::household_meta(household),
            keys::member(household, request.principal()),
        ];
        keys.extend_from_slice(extra);
        let mut snapshot = self.store.get_atomic(&keys).await?;
        let rest = snapshot.split_off(2);
        let membership = snapshot.pop().flatten();
        let meta = snapshot.pop().flatten().ok_or(Error::NotFound)?;
        let record = records::decode_household(&meta)?;
        if record.deleting || membership.is_none() {
            return Err(Error::NotFound);
        }
        if !record
            .household
            .role_of_member(request.principal())
            .allows(request.permission())
        {
            return Err(Error::Forbidden);
        }
        Ok((record, rest))
    }

    async fn invitations_of(&self, household: &HouseholdId) -> Result<Vec<Invitation>, Error> {
        let references = self
            .store
            .query(&keys::household_pk(household), Some(keys::INVITE_PREFIX))
            .await?
            .iter()
            .map(records::decode_invitation_ref)
            .collect::<Result<Vec<_>, _>>()?;
        let found = self
            .store
            .batch_get(
                &references
                    .iter()
                    .map(|reference| keys::invitation(&reference.hash))
                    .collect::<Vec<_>>(),
            )
            .await?;
        let mut invitations = Vec::new();
        for reference in &references {
            let pk = keys::invitation(&reference.hash).pk;
            let stored = found.iter().find(|item| {
                matches!(item.get(keys::PK), Some(aws_sdk_dynamodb::types::AttributeValue::S(value)) if *value == pk)
            });
            // References may dangle briefly while an invitation is being removed.
            if let Some(item) = stored {
                let invitation = records::decode_invitation(item, &reference.hash)?;
                if invitation.household_id() == household {
                    invitations.push(invitation);
                }
            }
        }
        invitations.sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        Ok(invitations)
    }

    async fn read_invitation(
        &self,
        hash: &InvitationTokenHash,
    ) -> Result<Option<Invitation>, Error> {
        Ok(self
            .store
            .get(&keys::invitation(hash))
            .await?
            .map(|item| records::decode_invitation(&item, hash))
            .transpose()?)
    }

    /// Removes every child record of a household marked `DELETING`, metadata last (ADR 017
    /// bounded deletion, step 2). Idempotent and safe to resume.
    async fn sweep_household(&self, household: &HouseholdId) -> Result<(), StoreError> {
        let rows = self
            .store
            .query(&keys::household_pk(household), None)
            .await?;
        let mut leftovers = Vec::new();
        for row in &rows {
            let Some(sk) = sort_key(row) else {
                return Err(StoreError);
            };
            if sk == keys::META {
                continue;
            }
            if sk.starts_with(keys::LIST_PREFIX) {
                self.sweep_list(&records::list_ref_target(row)?, false)
                    .await?;
            } else if sk.starts_with(keys::INVITE_PREFIX) {
                let reference = records::decode_invitation_ref(row)?;
                let key = keys::invitation(&reference.hash);
                if let Some(item) = self.store.get(&key).await? {
                    let invitation = records::decode_invitation(&item, &reference.hash)?;
                    self.store
                        .batch_delete(&[
                            key,
                            keys::user_created_invite(invitation.created_by(), &reference.hash),
                        ])
                        .await?;
                }
            }
            leftovers.extend(keys_of([row])?);
        }
        self.store.batch_delete(&leftovers).await?;
        // A cancelled delete means the metadata is already gone: the sweep is complete.
        self.store
            .commit(vec![Action::Delete {
                key: keys::household_meta(household),
                condition: Condition::present().equals(STATE, string(DELETING)),
            }])
            .await?;
        Ok(())
    }
}

impl HouseholdRepository for DynamoDbRepository {
    async fn create_household(&self, household: Household) -> Result<(), Error> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let snapshot = self
                .store
                .get_atomic(&[
                    keys::household_meta(household.id()),
                    keys::user_household_count(household.owner_id()),
                ])
                .await?;
            if snapshot[0].is_some() {
                return Err(Error::AlreadyExists);
            }
            let households = records::counter(snapshot[1].as_ref())?;
            if households >= Household::MAX_HOUSEHOLDS_PER_USER as u64 {
                return Err(Error::HouseholdLimitReached);
            }
            let record = HouseholdRecord {
                household: household.clone(),
                deleting: false,
                member_count: 1,
            };
            let mut actions = vec![put_household(&record, Condition::missing())];
            actions.extend(start_membership(household.id(), household.owner_id()));
            if self.store.commit(actions).await? == Commit::Committed {
                return Ok(());
            }
        }
        Err(Error::Unavailable)
    }

    async fn list_households(&self, principal: &UserId) -> Result<Vec<HouseholdSummary>, Error> {
        let references = self
            .store
            .query(&keys::user_pk(principal), Some(keys::HOUSEHOLD_PREFIX))
            .await?
            .iter()
            .map(records::household_ref_target)
            .collect::<Result<Vec<_>, _>>()?;
        let metas = self
            .store
            .batch_get(
                &references
                    .iter()
                    .map(keys::household_meta)
                    .collect::<Vec<_>>(),
            )
            .await?;
        let mut summaries = Vec::new();
        for meta in &metas {
            let record = records::decode_household(meta)?;
            if !record.deleting && references.contains(record.household.id()) {
                summaries.push(HouseholdSummary {
                    role: record.household.role_of_member(principal),
                    household: record.household,
                });
            }
        }
        summaries.sort_by(|left, right| {
            left.household
                .id()
                .as_str()
                .cmp(right.household.id().as_str())
        });
        Ok(summaries)
    }

    async fn get_household(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
    ) -> Result<HouseholdDetails, Error> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let (record, _) = self.authorize_household(access, household_id, &[]).await?;
            let rows = self
                .store
                .query(&keys::household_pk(household_id), Some(keys::MEMBER_PREFIX))
                .await?;
            // Membership changes advance the version: unchanged metadata proves a consistent
            // member list.
            if self.read_household(household_id).await?.as_ref() != Some(&record) {
                continue;
            }
            let mut members = rows
                .iter()
                .map(|row| {
                    records::member_user(row).map(|user_id| HouseholdMember {
                        role: record.household.role_of_member(&user_id),
                        user_id,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            members.sort_by(|left, right| left.user_id.as_str().cmp(right.user_id.as_str()));
            return Ok(HouseholdDetails {
                household: record.household,
                members,
            });
        }
        Err(Error::Unavailable)
    }

    async fn rename_household(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
        expected_version: AggregateVersion,
        name: HouseholdName,
    ) -> Result<Household, Error> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let (record, _) = self.authorize_household(access, household_id, &[]).await?;
            if record.household.version() != expected_version {
                return Err(Error::Conflict);
            }
            let mut next = record.clone();
            next.household
                .rename(name.clone())
                .map_err(version_exhausted)?;
            if next.household == record.household {
                return Ok(next.household);
            }
            let action = put_household(&next, meta_condition(&record, Some(access)));
            if self.store.commit(vec![action]).await? == Commit::Committed {
                return Ok(next.household);
            }
        }
        Err(Error::Unavailable)
    }

    async fn create_household_list(
        &self,
        access: &HouseholdAccessRequest,
        list: ShoppingList,
    ) -> Result<(), Error> {
        let household = list.household_id().cloned().ok_or(Error::InvalidRequest)?;
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let (_, extra) = self
                .authorize_household(
                    access,
                    &household,
                    &[
                        keys::list_meta(list.id()),
                        keys::household_list_count(&household),
                    ],
                )
                .await?;
            if extra[0].is_some() {
                return Err(Error::AlreadyExists);
            }
            if records::counter(extra[1].as_ref())? >= Household::MAX_LISTS as u64 {
                return Err(Error::ListLimitReached);
            }
            let actions = vec![
                Action::Put {
                    item: records::encode_list(&ListRecord::new(list.clone())),
                    condition: Condition::missing(),
                },
                Action::Put {
                    item: records::encode_list_ref(
                        keys::household_list_ref(&household, list.id()),
                        list.id(),
                    ),
                    condition: Condition::none(),
                },
                Action::Add {
                    key: keys::household_list_count(&household),
                    attribute: COUNT,
                    delta: 1,
                    condition: Condition::none().and(Check::MissingOrLessThan(
                        COUNT,
                        number(limit(Household::MAX_LISTS)),
                    )),
                },
                guard(&household, access),
            ];
            if self.store.commit(actions).await? == Commit::Committed {
                return Ok(());
            }
        }
        Err(Error::Unavailable)
    }

    async fn leave_household(
        &self,
        principal: &UserId,
        household_id: &HouseholdId,
    ) -> Result<Household, Error> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let snapshot = self
                .store
                .get_atomic(&[
                    keys::household_meta(household_id),
                    keys::member(household_id, principal),
                ])
                .await?;
            let record = match (&snapshot[0], &snapshot[1]) {
                (Some(meta), Some(_)) => records::decode_household(meta)?,
                _ => return Err(Error::NotFound),
            };
            if record.deleting {
                return Err(Error::NotFound);
            }
            if record.household.role_of_member(principal) == HouseholdRole::Owner {
                return Err(Error::OwnerMustTransferOwnership);
            }
            let mut next = record.clone();
            next.household
                .record_membership_change()
                .map_err(version_exhausted)?;
            next.member_count = next.member_count.saturating_sub(1);
            let mut actions = vec![put_household(&next, meta_condition(&record, None))];
            actions.extend(end_membership(household_id, principal));
            if self.store.commit(actions).await? == Commit::Committed {
                return Ok(next.household);
            }
        }
        Err(Error::Unavailable)
    }

    async fn remove_member(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
        expected_version: AggregateVersion,
        member: &UserId,
    ) -> Result<Household, Error> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let (record, extra) = self
                .authorize_household(access, household_id, &[keys::member(household_id, member)])
                .await?;
            if record.household.version() != expected_version {
                return Err(Error::Conflict);
            }
            if extra[0].is_none() {
                return Err(Error::NotFound);
            }
            if record.household.role_of_member(member) == HouseholdRole::Owner {
                return Err(Error::OwnerMustTransferOwnership);
            }
            let mut next = record.clone();
            next.household
                .record_membership_change()
                .map_err(version_exhausted)?;
            next.member_count = next.member_count.saturating_sub(1);
            let mut actions = vec![put_household(&next, meta_condition(&record, Some(access)))];
            actions.extend(end_membership(household_id, member));
            if self.store.commit(actions).await? == Commit::Committed {
                return Ok(next.household);
            }
        }
        Err(Error::Unavailable)
    }

    async fn transfer_ownership(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
        expected_version: AggregateVersion,
        new_owner: &UserId,
    ) -> Result<Household, Error> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let target = keys::member(household_id, new_owner);
            let (record, extra) = self
                .authorize_household(access, household_id, std::slice::from_ref(&target))
                .await?;
            if record.household.version() != expected_version {
                return Err(Error::Conflict);
            }
            if extra[0].is_none() {
                return Err(Error::NotFound);
            }
            let mut next = record.clone();
            next.household
                .transfer_ownership(new_owner.clone())
                .map_err(version_exhausted)?;
            if next.household == record.household {
                return Ok(next.household);
            }
            let actions = vec![
                put_household(&next, meta_condition(&record, Some(access))),
                Action::Check {
                    key: target,
                    condition: Condition::present(),
                },
            ];
            if self.store.commit(actions).await? == Commit::Committed {
                return Ok(next.household);
            }
        }
        Err(Error::Unavailable)
    }

    /// Step 1 (one transaction): owner and version check, mark `DELETING`, end every
    /// membership and free every member's household-limit slot. From then on the household,
    /// its lists and its invitations are inaccessible. Step 2 sweeps the children. A failed
    /// sweep reports `Unavailable`; the owner's retry resumes it and then reports `NotFound`.
    async fn delete_household(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
        expected_version: AggregateVersion,
    ) -> Result<(), Error> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            if let Some(current) = self.read_household(household_id).await?
                && current.deleting
            {
                if current.household.owner_id() == access.principal() {
                    self.sweep_household(household_id).await?;
                }
                return Err(Error::NotFound);
            }
            let (record, _) = self.authorize_household(access, household_id, &[]).await?;
            if record.household.version() != expected_version {
                return Err(Error::Conflict);
            }
            let members = self
                .store
                .query(&keys::household_pk(household_id), Some(keys::MEMBER_PREFIX))
                .await?
                .iter()
                .map(records::member_user)
                .collect::<Result<Vec<_>, _>>()?;
            if 1 + 3 * members.len() > MAX_TRANSACTION_ACTIONS {
                return Err(Error::Unavailable);
            }
            let mut marked = record.clone();
            marked.deleting = true;
            let mut actions = vec![put_household(
                &marked,
                meta_condition(&record, Some(access)),
            )];
            for member in &members {
                actions.extend(end_membership(household_id, member));
            }
            if self.store.commit(actions).await? == Commit::Committed {
                self.sweep_household(household_id).await?;
                return Ok(());
            }
        }
        Err(Error::Unavailable)
    }
}

impl InvitationRepository for DynamoDbRepository {
    async fn create_invitation(
        &self,
        access: &HouseholdAccessRequest,
        invitation: Invitation,
        now: Timestamp,
    ) -> Result<(), Error> {
        let household = invitation.household_id().clone();
        let reference = invitation_ref(&invitation);
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            // The revision is read before the invitations, so a concurrent creation that
            // commits in between cancels this transaction.
            let rev_key = keys::household_invite_rev(&household);
            let household_ref = keys::household_invite_ref(&household, invitation.id());
            let (_, extra) = self
                .authorize_household(
                    access,
                    &household,
                    &[
                        rev_key.clone(),
                        keys::invitation(invitation.token_hash()),
                        household_ref.clone(),
                    ],
                )
                .await?;
            if invitation.created_by() != access.principal() {
                return Err(Error::InvalidRequest);
            }
            let valid = self
                .invitations_of(&household)
                .await?
                .iter()
                .filter(|candidate| candidate.is_valid_at(now))
                .count();
            if valid >= Invitation::MAX_VALID_PER_HOUSEHOLD {
                return Err(Error::InvitationLimitReached);
            }
            if extra[1].is_some() || extra[2].is_some() {
                return Err(Error::AlreadyExists);
            }
            let revision = records::revision(extra[0].as_ref())?;
            let rev_condition = match revision {
                None => Condition::missing(),
                Some(rev) => Condition::present().equals(REV, number(rev)),
            };
            let actions = vec![
                Action::Put {
                    item: records::encode_invitation(&invitation),
                    condition: Condition::missing(),
                },
                Action::Put {
                    item: records::encode_invitation_ref(household_ref, &reference),
                    condition: Condition::missing(),
                },
                Action::Put {
                    item: records::encode_invitation_ref(
                        keys::user_created_invite(invitation.created_by(), invitation.token_hash()),
                        &reference,
                    ),
                    condition: Condition::none(),
                },
                Action::Put {
                    item: records::encode_revision(rev_key, revision.map_or(1, |rev| rev + 1)),
                    condition: rev_condition,
                },
                guard(&household, access),
            ];
            if self.store.commit(actions).await? == Commit::Committed {
                return Ok(());
            }
        }
        Err(Error::Unavailable)
    }

    async fn list_invitations(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
    ) -> Result<Vec<Invitation>, Error> {
        self.authorize_household(access, household_id, &[]).await?;
        self.invitations_of(household_id).await
    }

    async fn revoke_invitation(
        &self,
        access: &HouseholdAccessRequest,
        household_id: &HouseholdId,
        invitation_id: &InvitationId,
    ) -> Result<Invitation, Error> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let (_, extra) = self
                .authorize_household(
                    access,
                    household_id,
                    &[keys::household_invite_ref(household_id, invitation_id)],
                )
                .await?;
            let reference =
                records::decode_invitation_ref(extra[0].as_ref().ok_or(Error::NotFound)?)?;
            let current = self
                .read_invitation(&reference.hash)
                .await?
                .ok_or(Error::NotFound)?;
            if current.household_id() != household_id || current.id() != invitation_id {
                return Err(Error::NotFound);
            }
            if current.is_revoked() {
                return Ok(current);
            }
            let mut revoked = current.clone();
            revoked.revoke();
            let actions = vec![
                Action::Put {
                    item: records::encode_invitation(&revoked),
                    condition: Condition::present()
                        .equals(USED_COUNT, number(current.used_count()))
                        .equals(
                            REVOKED,
                            aws_sdk_dynamodb::types::AttributeValue::Bool(false),
                        ),
                },
                guard(household_id, access),
            ];
            if self.store.commit(actions).await? == Commit::Committed {
                return Ok(revoked);
            }
        }
        Err(Error::Unavailable)
    }

    async fn accept_invitation(
        &self,
        principal: &UserId,
        token_hash: &InvitationTokenHash,
        now: Timestamp,
    ) -> Result<HouseholdSummary, Error> {
        for attempt in 0..MAX_ATTEMPTS {
            self.store.pause(attempt).await;
            let current = self
                .read_invitation(token_hash)
                .await?
                .ok_or(Error::InvitationUnavailable)?;
            if current.check_usable(now).is_err() {
                return Err(Error::InvitationUnavailable);
            }
            let household_id = current.household_id().clone();
            let snapshot = self
                .store
                .get_atomic(&[
                    keys::household_meta(&household_id),
                    keys::member(&household_id, principal),
                    keys::user_household_count(principal),
                ])
                .await?;
            let Some(meta) = &snapshot[0] else {
                return Err(Error::InvitationUnavailable);
            };
            let record = records::decode_household(meta)?;
            if record.deleting {
                return Err(Error::InvitationUnavailable);
            }
            if snapshot[1].is_some() {
                return Err(Error::AlreadyMember);
            }
            if record.member_count as usize >= Household::MAX_MEMBERS {
                return Err(Error::MemberLimitReached);
            }
            if records::counter(snapshot[2].as_ref())? >= Household::MAX_HOUSEHOLDS_PER_USER as u64
            {
                return Err(Error::HouseholdLimitReached);
            }
            // Every fallible step happens before the transaction, which commits the use, the
            // membership and the household version together.
            let mut next = record.clone();
            next.household
                .record_membership_change()
                .map_err(version_exhausted)?;
            next.member_count += 1;
            let mut used = current.clone();
            used.record_use(now)
                .map_err(|_| Error::InvitationUnavailable)?;
            let mut actions = vec![
                Action::Put {
                    item: records::encode_invitation(&used),
                    condition: Condition::present()
                        .equals(USED_COUNT, number(current.used_count()))
                        .equals(
                            REVOKED,
                            aws_sdk_dynamodb::types::AttributeValue::Bool(false),
                        ),
                },
                put_household(&next, meta_condition(&record, None)),
            ];
            actions.extend(start_membership(&household_id, principal));
            if self.store.commit(actions).await? == Commit::Committed {
                return Ok(HouseholdSummary {
                    household: next.household,
                    role: HouseholdRole::Member,
                });
            }
        }
        Err(Error::Unavailable)
    }

    /// Removes the invitations the principal created, found through the creator references
    /// (`USER#p / INVITE#hash`); no index or scan is needed (roadmap D2).
    async fn delete_invitations_created_by(&self, principal: &UserId) -> Result<(), Error> {
        let references = self
            .store
            .query(&keys::user_pk(principal), Some(keys::INVITE_PREFIX))
            .await?
            .iter()
            .map(records::decode_invitation_ref)
            .collect::<Result<Vec<_>, _>>()?;
        for reference in references {
            let mut removed = false;
            for attempt in 0..MAX_ATTEMPTS {
                self.store.pause(attempt).await;
                let invitation_key = keys::invitation(&reference.hash);
                let household_ref =
                    keys::household_invite_ref(&reference.household, &reference.invitation);
                let snapshot = self
                    .store
                    .get_atomic(&[invitation_key.clone(), household_ref.clone()])
                    .await?;
                let mut actions = Vec::new();
                if let Some(item) = &snapshot[0]
                    && records::decode_invitation(item, &reference.hash)?.created_by() == principal
                {
                    actions.push(Action::Delete {
                        key: invitation_key,
                        condition: Condition::present()
                            .equals(CREATED_BY, string(principal.as_str())),
                    });
                }
                if let Some(item) = &snapshot[1]
                    && records::decode_invitation_ref(item)?.hash == reference.hash
                {
                    actions.push(Action::Delete {
                        key: household_ref,
                        condition: Condition::present()
                            .equals(TOKEN_HASH, string(keys::hash_hex(&reference.hash))),
                    });
                }
                actions.push(Action::Delete {
                    key: keys::user_created_invite(principal, &reference.hash),
                    condition: Condition::none(),
                });
                if self.store.commit(actions).await? == Commit::Committed {
                    removed = true;
                    break;
                }
            }
            if !removed {
                return Err(Error::Unavailable);
            }
        }
        Ok(())
    }
}
