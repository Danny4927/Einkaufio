variable "name_prefix" {
  description = "Prefix for alarm, topic and budget names."
  type        = string
}

variable "lambda_function_name" {
  description = "Lambda function to monitor."
  type        = string
}

variable "dynamodb_table_name" {
  description = "DynamoDB table to monitor."
  type        = string
}

variable "alert_emails" {
  description = "Email recipients for alarms and budget notifications."
  type        = list(string)

  validation {
    condition     = length(var.alert_emails) > 0
    error_message = "At least one alert email is required."
  }
}

variable "monthly_budget_usd" {
  description = "Monthly cost budget in USD."
  type        = number
  default     = 5

  validation {
    condition     = var.monthly_budget_usd > 0
    error_message = "monthly_budget_usd must be positive."
  }
}

variable "tags" {
  description = "Additional tags."
  type        = map(string)
  default     = {}
}
