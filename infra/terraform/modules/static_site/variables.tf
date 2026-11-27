variable "bucket_name" {
  description = "Private S3 bucket name for web assets."
  type        = string
}

variable "content_security_policy" {
  description = "Content-Security-Policy header value for the web app."
  type        = string
}

variable "tags" {
  description = "Additional tags."
  type        = map(string)
  default     = {}
}
