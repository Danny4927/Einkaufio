output "function_name" {
  description = "Lambda function name."
  value       = aws_lambda_function.this.function_name
}

output "function_url" {
  description = "HTTPS Function URL."
  value       = aws_lambda_function_url.this.function_url
}

output "role_arn" {
  description = "Execution role ARN."
  value       = aws_iam_role.this.arn
}
