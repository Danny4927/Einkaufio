output "user_pool_id" {
  description = "User pool ID."
  value       = aws_cognito_user_pool.this.id
}

output "user_pool_arn" {
  description = "User pool ARN."
  value       = aws_cognito_user_pool.this.arn
}

output "issuer" {
  description = "Expected JWT issuer for access tokens."
  value       = "https://${aws_cognito_user_pool.this.endpoint}"
}

output "public_client_id" {
  description = "Public (PKCE) app client ID."
  value       = aws_cognito_user_pool_client.public.id
}

output "hosted_domain" {
  description = "Cognito hosted domain prefix."
  value       = aws_cognito_user_pool_domain.this.domain
}
