# Shopping item implementation progress

The first item vertical slice implements:
- Item IDs and validated name, quantity, unit and note value objects.
- New items start unchecked at item version 1.
- Check/uncheck is idempotent and increments the item version only on a state change.
- Application use cases add, check/uncheck and delete items using application-owned ports.
- Every mutation receives the authenticated actor identity, derived from verified request context.
- Repository adapters must authorize the actor before revealing list/item existence.
- Add requires an expected list version; check and delete require an expected item version.
- Adapter mutations must be atomic with the parent-list version update. Add must enforce the 100-active-item limit and assign a unique position atomically.
- Check returns the updated item (including its new item version) plus the new parent-list version. Add/delete return the new parent-list version. These versions are intentionally distinct.

Not implemented: concrete adapter, HTTP endpoints, update/reorder use cases, read/list pagination, or browser UI. The use cases are not deployable API features until those adapters are implemented.
