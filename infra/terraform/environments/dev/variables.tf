variable "region" {
  description = "AWS region."
  type        = string
  default     = "eu-central-1"
}

variable "name_prefix" {
  description = "Resource name prefix."
  type        = string
  default     = "einkaufio-dev"
}

variable "lambda_package_path" {
  description = "Path to the built API zip (bootstrap binary, aarch64)."
  type        = string
}

variable "web_bucket_name" {
  description = "Globally unique bucket name for web assets."
  type        = string
}

variable "cognito_domain_prefix" {
  description = "Globally unique Cognito hosted domain prefix."
  type        = string
}

variable "web_origins" {
  description = "Web origins allowed for CORS and OAuth redirects (e.g. the CloudFront URL, http://localhost:4200)."
  type        = list(string)
}

variable "alert_emails" {
  description = "Recipients for alarms and budget notifications."
  type        = list(string)
}

variable "monthly_budget_usd" {
  description = "Monthly cost budget in USD."
  type        = number
  default     = 5
}

variable "lambda_reserved_concurrency" {
  description = "Reserved Lambda concurrency (cost/abuse guard)."
  type        = number
  default     = 5
}
