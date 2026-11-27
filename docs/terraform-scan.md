# Terraform static security scan

`make terraform-scan` runs [Checkov](https://www.checkov.io/) (`CHECKOV ?= checkov`,
reviewed with 3.2.469) over `infra/terraform` with `--skip-download` (no
Prisma Cloud calls). Trivy was preferred but its release binaries were not
reachable from the authoring environment; Checkov installs from PyPI
(`uv tool install checkov==3.2.469` or `pipx install checkov==3.2.469`).

The scan fails on any unreviewed finding. Accepted findings are suppressed
inline with `# checkov:skip=<ID>:<reason>` next to the resource, so each
suppression is visible in review. Revisit this list whenever a module changes,
before prod, and when data classification changes.

## Reviewed findings (last review: 2026-11-27, 96 passed, 0 failed, 27 skipped)

| Check | Resource | Decision |
| --- | --- | --- |
| CKV_AWS_259 | CloudFront response headers policy | Fixed: HSTS with `preload`. |
| CKV_AWS_18, CKV_AWS_86 | State/site buckets, CloudFront | Access logging deferred for cost; CloudTrail management events cover the state bucket. Revisit before prod. |
| CKV_AWS_144 | State/site buckets | No cross-region replication; state is versioned, site assets are rebuilt from git. |
| CKV_AWS_145, CKV_AWS_119, CKV_AWS_158, CKV_AWS_173, CKV_AWS_26 | S3, DynamoDB, log group, Lambda env, SNS | AWS-managed/owned encryption instead of customer KMS keys (cost). No secrets in state, env vars or logs by policy (docs/security.md). Revisit if data classification changes. |
| CKV2_AWS_62 | Buckets | No S3 event consumers. |
| CKV_AWS_21, CKV2_AWS_61 | Site bucket | Static assets are redeployed from CI; versioning/lifecycle add no recovery value. |
| CKV_AWS_338 | Lambda log group | 14-day retention is intentional (cost, data minimisation). |
| CKV_AWS_116 | Lambda | DLQ irrelevant for synchronous Function URL invocations. |
| CKV_AWS_117 | Lambda | No VPC/NAT in v1 (ADR 001). |
| CKV_AWS_272 | Lambda | Code signing deferred until CI publishes signed artifacts. |
| CKV_AWS_50 | Lambda | X-Ray deferred for cost; error/throttle alarms in place. |
| CKV_AWS_258 | Function URL | `AuthType=NONE` accepted: public clients cannot SigV4-sign; Cognito JWT validation in the API (arc42 section 8, docs/security.md), reserved concurrency and CORS allowlist as guards. |
| CKV_AWS_68, CKV2_AWS_47 | CloudFront | No WAF (recurring cost); CloudFront serves static assets only. |
| CKV_AWS_310 | CloudFront | Single origin; failover not justified. |
| CKV_AWS_374 | CloudFront | Geo restriction is not a requirement. |
| CKV2_AWS_42, CKV_AWS_174 | CloudFront | Default `*.cloudfront.net` certificate until a custom domain exists; then require TLSv1.2_2021. |
