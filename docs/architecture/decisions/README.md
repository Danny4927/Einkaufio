# Architecture Decision Records

| ADR | Decision | Status |
|---|---|---|
| [001](001-serverless-aws-and-modular-monolith.md) | Serverless AWS and modular monolith | Accepted |
| [002](002-cognito-authentication.md) | Cognito for user accounts | Accepted |
| [003](003-dynamodb-single-table.md) | DynamoDB single-table design | Accepted |
| [004](004-polling-synchronization.md) | Polling synchronization | Accepted |
| [005](005-personal-lists-and-households.md) | Personal lists and households | Accepted |
| [006](006-platform-neutral-api-android.md) | Platform-neutral API and future Android | Accepted |
| [007](007-angular-web-hosting.md) | Angular on S3 and CloudFront | Accepted |
| [008](008-invitation-links.md) | Secure invitation links | Accepted |
| [009](009-ddd-hexagonal-and-swappable-persistence.md) | DDD, hexagonal architecture and replaceable persistence | Accepted |
| [010](010-evidence-based-delivery-and-visual-regression.md) | Evidence-based delivery and visual regression | Accepted |
| [011](011-monorepo-terraform-and-enforced-architecture.md) | Monorepo, Terraform IaC and enforceable architecture | Accepted |
| [012](012-agent-workflow-and-enforced-quality-gates.md) | Agent workflow and enforced quality gates | Accepted |
| [013](013-walking-skeleton-and-vertical-delivery-phases.md) | Walking skeleton first and vertical delivery phases | Accepted |
| [014](014-async-repository-ports.md) | Asynchronous repository ports | Accepted |
| [015](015-list-access-model.md) | List access model (owner/household; explicit sharing in v1.x) | Accepted |
| [016](016-per-principal-throttling.md) | Per-principal request throttling (best effort per Lambda instance) | Accepted |
| [017](017-households-invitation-token-ports.md) | Households, membership lifecycle and invitation token ports | Accepted |
| [018](018-household-http-invitation-urls-and-account-data.md) | Household HTTP routes, invitation URLs, account deletion and data export | Accepted |
| [019](019-dynamodb-adapter-design.md) | DynamoDB adapter: item index in list metadata, optimistic transactions, counters, bounded sweeps | Accepted |
| [020](020-web-client-auth-runtime-config-and-sync.md) | Web client: Cognito code + PKCE, runtime config, visible-list polling with optimistic UI | Accepted |

ADRs record decisions and their trade-offs. Change accepted decisions through a superseding ADR rather than silently changing the architecture.
