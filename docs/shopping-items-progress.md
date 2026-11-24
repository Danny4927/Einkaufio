# Shopping item implementation progress

The first item vertical slice implements:
- Item IDs and validated name, quantity, unit and note value objects.
- New items start unchecked at item version 1.
- Check/uncheck is idempotent and increments the item version only on a state change.
- Application use cases add, check/uncheck and delete items using application-owned ports.
- Add requires an expected list version; check and delete require an expected item version.
- Adapters must atomically persist an item mutation and increment the parent list version. The adapter must also enforce authorization and the 100-active-item limit; no DynamoDB types are present in domain/application.

Not implemented: concrete adapter, HTTP endpoints, update/reorder use cases, read/list pagination, or browser UI. The use cases are not deployable API features until those adapters are implemented.
