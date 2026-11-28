resource "aws_cloudwatch_log_group" "this" {
  # checkov:skip=CKV_AWS_158:Log groups use the AWS-managed key; no KMS cost until logs contain regulated data. Tokens are never logged.
  # checkov:skip=CKV_AWS_338:Retention is deliberately short (cost and data minimisation); configurable per environment.
  name              = "/aws/lambda/${var.function_name}"
  retention_in_days = var.log_retention_days
  tags              = var.tags
}

data "aws_iam_policy_document" "assume" {
  statement {
    actions = ["sts:AssumeRole"]

    principals {
      type        = "Service"
      identifiers = ["lambda.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "this" {
  name               = "${var.function_name}-exec"
  assume_role_policy = data.aws_iam_policy_document.assume.json
  tags               = var.tags
}

# Least privilege: write only to its own log group and access only the one
# table (item CRUD, query, transactions). No Scan, no table management.
data "aws_iam_policy_document" "this" {
  statement {
    sid       = "OwnLogs"
    actions   = ["logs:CreateLogStream", "logs:PutLogEvents"]
    resources = ["${aws_cloudwatch_log_group.this.arn}:*"]
  }

  statement {
    sid = "TableAccess"
    actions = [
      "dynamodb:GetItem",
      "dynamodb:PutItem",
      "dynamodb:UpdateItem",
      "dynamodb:DeleteItem",
      "dynamodb:Query",
      "dynamodb:BatchGetItem",
      # Bounded deletion sweeps (ADR 019).
      "dynamodb:BatchWriteItem",
      "dynamodb:ConditionCheckItem",
    ]
    resources = [
      var.dynamodb_table_arn,
      "${var.dynamodb_table_arn}/index/*",
    ]
  }
}

resource "aws_iam_role_policy" "this" {
  name   = "${var.function_name}-least-privilege"
  role   = aws_iam_role.this.id
  policy = data.aws_iam_policy_document.this.json
}

resource "aws_lambda_function" "this" {
  # checkov:skip=CKV_AWS_116:Synchronous Function URL invocation; a DLQ only applies to async invocations.
  # checkov:skip=CKV_AWS_117:ADR 001 excludes VPC/NAT in v1; the function only calls public AWS endpoints.
  # checkov:skip=CKV_AWS_173:Environment holds only non-secret configuration (table name, issuer, client ID); AWS-managed key is sufficient.
  # checkov:skip=CKV_AWS_272:Code signing is deferred until a CI deploy pipeline publishes signed artifacts.
  # checkov:skip=CKV_AWS_50:X-Ray tracing is deferred for cost; CloudWatch alarms cover errors/throttles.
  function_name                  = var.function_name
  role                           = aws_iam_role.this.arn
  runtime                        = "provided.al2023"
  handler                        = "bootstrap"
  architectures                  = ["arm64"]
  filename                       = var.package_path
  source_code_hash               = filebase64sha256(var.package_path)
  memory_size                    = var.memory_size
  timeout                        = var.timeout_seconds
  reserved_concurrent_executions = var.reserved_concurrency

  environment {
    variables = var.environment
  }

  logging_config {
    log_format = "JSON"
    log_group  = aws_cloudwatch_log_group.this.name
  }

  depends_on = [aws_iam_role_policy.this]

  tags = var.tags
}

# AuthType NONE is an accepted decision (arc42 section 8, docs/security.md):
# every application route validates Cognito JWTs in the Rust API.
resource "aws_lambda_function_url" "this" {
  # checkov:skip=CKV_AWS_258:Public clients cannot SigV4-sign; JWT validation is enforced in the application (docs/security.md).
  function_name      = aws_lambda_function.this.function_name
  authorization_type = "NONE"

  cors {
    allow_origins  = var.cors_allowed_origins
    allow_methods  = ["GET", "POST", "PUT", "PATCH", "DELETE"]
    allow_headers  = ["authorization", "content-type", "if-match"]
    expose_headers = ["etag"]
    max_age        = 600
  }
}
