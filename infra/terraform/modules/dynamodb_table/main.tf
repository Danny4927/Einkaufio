resource "aws_dynamodb_table" "this" {
  # checkov:skip=CKV_AWS_119:AWS-owned key encryption is used to avoid KMS cost; revisit before storing data beyond shopping lists (docs/terraform-scan.md).
  name         = var.name
  billing_mode = "PAY_PER_REQUEST"
  hash_key     = "PK"
  range_key    = "SK"

  attribute {
    name = "PK"
    type = "S"
  }

  attribute {
    name = "SK"
    type = "S"
  }

  point_in_time_recovery {
    enabled = true
  }

  server_side_encryption {
    enabled = true
  }

  deletion_protection_enabled = var.deletion_protection

  tags = var.tags
}
