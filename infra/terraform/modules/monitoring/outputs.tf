output "alerts_topic_arn" {
  description = "SNS topic receiving alarm notifications."
  value       = aws_sns_topic.alerts.arn
}
