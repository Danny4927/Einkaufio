variable "region" {
  description = "AWS region of the state bucket."
  type        = string
  default     = "eu-central-1"
}

variable "state_bucket_name" {
  description = "Globally unique name of the Terraform state bucket (e.g. einkaufio-tfstate-<account-id>)."
  type        = string

  validation {
    condition     = can(regex("^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$", var.state_bucket_name))
    error_message = "state_bucket_name must be a valid S3 bucket name."
  }
}

variable "noncurrent_version_retention_days" {
  description = "Days to keep noncurrent state object versions."
  type        = number
  default     = 90

  validation {
    condition     = var.noncurrent_version_retention_days >= 30
    error_message = "Keep noncurrent state versions for at least 30 days."
  }
}
