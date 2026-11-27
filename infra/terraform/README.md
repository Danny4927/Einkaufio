# Terraform infrastructure

Terraform is the source of truth for AWS resources.

- `bootstrap/`: isolated root for creating and protecting the remote state bucket.
- `environments/dev/` and `environments/prod/`: separate state roots.
- `modules/`: reusable AWS building blocks.

Modules: `dynamodb_table`, `cognito_auth`, `lambda_api`, `static_site`,
`monitoring`. The dev root composes them. Nothing has been applied yet; see
`docs/runbooks/terraform-bootstrap.md` for the bootstrap/migration procedure and
`docs/terraform-scan.md` for the reviewed static-scan findings (`make terraform-scan`).
Deploy IAM/OIDC roles and the prod root are still open.

Use Terraform 1.10 or newer for S3 native state locking. Never commit state files,
plan files containing sensitive values, credentials or secret tfvars.
