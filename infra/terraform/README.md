# Terraform infrastructure

Terraform is the source of truth for AWS resources.

- `bootstrap/`: isolated root for creating and protecting the remote state bucket.
- `environments/dev/` and `environments/prod/`: separate state roots.
- `modules/`: reusable AWS building blocks.

No AWS resources are provisioned by this skeleton. Backend configuration, providers,
IAM/OIDC roles and resources are added in the infrastructure milestone after review
of the target AWS account, region, costs and deployment permissions.

Use Terraform 1.10 or newer for S3 native state locking. Never commit state files,
plan files containing sensitive values, credentials or secret tfvars.
