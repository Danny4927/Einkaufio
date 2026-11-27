variable "name" {
  description = "Table name."
  type        = string
}

variable "deletion_protection" {
  description = "Enable DynamoDB deletion protection."
  type        = bool
  default     = true
}

variable "tags" {
  description = "Additional tags."
  type        = map(string)
  default     = {}
}
