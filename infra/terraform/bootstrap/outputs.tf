output "state_bucket_name" {
  description = "Name of the Terraform remote state bucket."
  value       = aws_s3_bucket.state.bucket
}

output "state_bucket_arn" {
  description = "ARN of the Terraform remote state bucket."
  value       = aws_s3_bucket.state.arn
}
