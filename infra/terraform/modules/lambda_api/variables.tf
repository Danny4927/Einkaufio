variable "function_name" {
  description = "Lambda function name."
  type        = string
}

variable "package_path" {
  description = "Path to the zip containing the `bootstrap` binary built for aarch64 Linux."
  type        = string
}

variable "dynamodb_table_arn" {
  description = "ARN of the single application table."
  type        = string
}

variable "environment" {
  description = "Non-secret environment variables for the function."
  type        = map(string)
  default     = {}
}

variable "memory_size" {
  description = "Memory in MB."
  type        = number
  default     = 256
}

variable "timeout_seconds" {
  description = "Function timeout in seconds."
  type        = number
  default     = 10
}

variable "reserved_concurrency" {
  description = "Reserved concurrency; first cost/abuse guard. Must be at least 1 (0 disables the function)."
  type        = number
  default     = 5

  validation {
    condition     = var.reserved_concurrency >= 1 && var.reserved_concurrency <= 50
    error_message = "reserved_concurrency must be between 1 and 50."
  }
}

variable "log_retention_days" {
  description = "CloudWatch log retention in days."
  type        = number
  default     = 14

  validation {
    condition     = contains([1, 3, 5, 7, 14, 30, 60, 90, 120, 150, 180, 365], var.log_retention_days)
    error_message = "log_retention_days must be a supported CloudWatch value up to 365."
  }
}

variable "cors_allowed_origins" {
  description = "Exact origins allowed to call the Function URL (no wildcard)."
  type        = list(string)

  validation {
    condition     = length(var.cors_allowed_origins) > 0 && !contains(var.cors_allowed_origins, "*")
    error_message = "Provide explicit CORS origins; '*' is not allowed."
  }
}

variable "tags" {
  description = "Additional tags."
  type        = map(string)
  default     = {}
}
