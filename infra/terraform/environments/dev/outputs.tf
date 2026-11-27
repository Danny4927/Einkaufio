output "api_function_url" {
  description = "API Function URL."
  value       = module.api.function_url
}

output "web_domain_name" {
  description = "CloudFront domain for the web app."
  value       = module.web.distribution_domain_name
}

output "cognito_user_pool_id" {
  description = "Cognito user pool ID."
  value       = module.auth.user_pool_id
}

output "cognito_public_client_id" {
  description = "Cognito public client ID."
  value       = module.auth.public_client_id
}

output "table_name" {
  description = "DynamoDB table name."
  value       = module.table.name
}
