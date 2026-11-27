variable "name" {
  description = "User pool name."
  type        = string
}

variable "domain_prefix" {
  description = "Cognito hosted domain prefix (globally unique per region)."
  type        = string
}

variable "callback_urls" {
  description = "Allowed OAuth redirect URIs for the public client (HTTPS or native app schemes)."
  type        = list(string)

  validation {
    condition     = length(var.callback_urls) > 0
    error_message = "At least one callback URL is required."
  }
}

variable "logout_urls" {
  description = "Allowed sign-out redirect URIs."
  type        = list(string)
}

variable "deletion_protection" {
  description = "Enable user pool deletion protection."
  type        = bool
  default     = true
}

variable "tags" {
  description = "Additional tags."
  type        = map(string)
  default     = {}
}
