resource "aws_sns_topic" "alerts" {
  # checkov:skip=CKV_AWS_26:Alert topic carries only alarm metadata; AWS-managed encryption avoids KMS key policy for CloudWatch/Budgets publishers.
  name = "${var.name_prefix}-alerts"
  tags = var.tags
}

resource "aws_sns_topic_subscription" "email" {
  for_each  = toset(var.alert_emails)
  topic_arn = aws_sns_topic.alerts.arn
  protocol  = "email"
  endpoint  = each.value
}

locals {
  alarm_actions = [aws_sns_topic.alerts.arn]

  lambda_alarms = {
    errors    = "Errors"
    throttles = "Throttles"
  }

  dynamodb_alarms = {
    read-throttles  = "ReadThrottleEvents"
    write-throttles = "WriteThrottleEvents"
  }
}

resource "aws_cloudwatch_metric_alarm" "lambda" {
  for_each = local.lambda_alarms

  alarm_name          = "${var.name_prefix}-lambda-${each.key}"
  alarm_description   = "Lambda ${each.value} for ${var.lambda_function_name}"
  namespace           = "AWS/Lambda"
  metric_name         = each.value
  dimensions          = { FunctionName = var.lambda_function_name }
  statistic           = "Sum"
  period              = 300
  evaluation_periods  = 1
  threshold           = 0
  comparison_operator = "GreaterThanThreshold"
  treat_missing_data  = "notBreaching"
  alarm_actions       = local.alarm_actions
  ok_actions          = local.alarm_actions
  tags                = var.tags
}

resource "aws_cloudwatch_metric_alarm" "dynamodb" {
  for_each = local.dynamodb_alarms

  alarm_name          = "${var.name_prefix}-dynamodb-${each.key}"
  alarm_description   = "DynamoDB ${each.value} on ${var.dynamodb_table_name}"
  namespace           = "AWS/DynamoDB"
  metric_name         = each.value
  dimensions          = { TableName = var.dynamodb_table_name }
  statistic           = "Sum"
  period              = 300
  evaluation_periods  = 1
  threshold           = 0
  comparison_operator = "GreaterThanThreshold"
  treat_missing_data  = "notBreaching"
  alarm_actions       = local.alarm_actions
  ok_actions          = local.alarm_actions
  tags                = var.tags
}

# Budgets notify; they are not a hard spend cap (arc42 section 10).
resource "aws_budgets_budget" "monthly" {
  name         = "${var.name_prefix}-monthly"
  budget_type  = "COST"
  limit_amount = tostring(var.monthly_budget_usd)
  limit_unit   = "USD"
  time_unit    = "MONTHLY"

  notification {
    comparison_operator        = "GREATER_THAN"
    threshold                  = 80
    threshold_type             = "PERCENTAGE"
    notification_type          = "ACTUAL"
    subscriber_email_addresses = var.alert_emails
  }

  notification {
    comparison_operator        = "GREATER_THAN"
    threshold                  = 100
    threshold_type             = "PERCENTAGE"
    notification_type          = "FORECASTED"
    subscriber_email_addresses = var.alert_emails
  }
}
