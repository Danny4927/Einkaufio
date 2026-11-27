# Runbook: Terraform state bootstrap and dev root

Status: prepared, not applied. Requires a reviewed AWS account, admin
credentials outside the repository and Terraform >= 1.10 (S3 native locking).

## 1. Bootstrap the state bucket (local state first)

```sh
cd infra/terraform/bootstrap
terraform init
terraform plan  -var state_bucket_name=einkaufio-tfstate-<account-id> -out bootstrap.tfplan
terraform apply bootstrap.tfplan
```

The bucket has versioning, SSE-S3, a full public access block,
`BucketOwnerEnforced`, a TLS-only bucket policy, noncurrent-version expiry and
`prevent_destroy`.

## 2. Migrate bootstrap state into the bucket

```sh
cp ../backend.hcl.example ../backend.hcl   # set bucket; backend.hcl is git-ignored
cp backend.tf.example backend.tf
terraform init -migrate-state -backend-config=../backend.hcl
rm -f terraform.tfstate terraform.tfstate.backup   # only after verifying remote state
```

Commit `backend.tf` afterwards. State keys: `einkaufio/bootstrap/terraform.tfstate`,
`einkaufio/dev/terraform.tfstate` (lock files `<key>.tflock` via `use_lockfile`).

## 3. Dev root

```sh
cd infra/terraform/environments/dev
cp terraform.tfvars.example terraform.tfvars   # git-ignored
terraform init -backend-config=../../backend.hcl
terraform plan -out dev.tfplan
```

The Lambda zip (`bootstrap` built for aarch64 Linux) must exist at
`lambda_package_path`. Confirm the SNS email subscriptions after apply.

## Provider lock file

Root lock files (`bootstrap/`, `environments/dev/`) pin `hashicorp/aws` 6.72.0
with `zh:` hashes for linux/darwin/windows amd64/arm64 taken from the
HashiCorp release `SHA256SUMS`. To upgrade:
`terraform providers lock -platform=linux_amd64 -platform=linux_arm64 -platform=darwin_amd64 -platform=darwin_arm64 -platform=windows_amd64`.
Module directories do not commit lock files.
