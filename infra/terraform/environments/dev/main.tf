module "table" {
  source = "../../modules/dynamodb_table"
  name   = "${var.name_prefix}-main"
}

module "auth" {
  source        = "../../modules/cognito_auth"
  name          = var.name_prefix
  domain_prefix = var.cognito_domain_prefix
  callback_urls = [for origin in var.web_origins : "${origin}/auth/callback"]
  logout_urls   = [for origin in var.web_origins : "${origin}/"]
}

module "api" {
  source               = "../../modules/lambda_api"
  function_name        = "${var.name_prefix}-api"
  package_path         = var.lambda_package_path
  dynamodb_table_arn   = module.table.arn
  reserved_concurrency = var.lambda_reserved_concurrency
  log_retention_days   = 14
  cors_allowed_origins = var.web_origins

  environment = {
    TABLE_NAME        = module.table.name
    COGNITO_ISSUER    = module.auth.issuer
    COGNITO_CLIENT_ID = module.auth.public_client_id
  }
}

module "web" {
  source                  = "../../modules/static_site"
  bucket_name             = var.web_bucket_name
  content_security_policy = "default-src 'self'; connect-src 'self' https://*.lambda-url.${var.region}.on.aws https://cognito-idp.${var.region}.amazonaws.com https://${var.cognito_domain_prefix}.auth.${var.region}.amazoncognito.com; img-src 'self' data:; style-src 'self'; script-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'"
}

module "monitoring" {
  source               = "../../modules/monitoring"
  name_prefix          = var.name_prefix
  lambda_function_name = module.api.function_name
  dynamodb_table_name  = module.table.name
  alert_emails         = var.alert_emails
  monthly_budget_usd   = var.monthly_budget_usd
}
