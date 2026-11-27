resource "aws_s3_bucket" "site" {
  # checkov:skip=CKV_AWS_18:Access logging deferred for cost at dev scale (docs/terraform-scan.md).
  # checkov:skip=CKV_AWS_144:Build artifacts are reproducible from git; no replication needed.
  # checkov:skip=CKV_AWS_145:SSE-S3 is sufficient for public static assets.
  # checkov:skip=CKV2_AWS_62:No event consumers.
  # checkov:skip=CKV_AWS_21:Assets are redeployed from CI; versioning adds cost without recovery value.
  # checkov:skip=CKV2_AWS_61:Lifecycle rules not needed for redeployed static assets.
  bucket        = var.bucket_name
  force_destroy = false
  tags          = var.tags
}

resource "aws_s3_bucket_ownership_controls" "site" {
  bucket = aws_s3_bucket.site.id

  rule {
    object_ownership = "BucketOwnerEnforced"
  }
}

resource "aws_s3_bucket_public_access_block" "site" {
  bucket = aws_s3_bucket.site.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_server_side_encryption_configuration" "site" {
  bucket = aws_s3_bucket.site.id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_cloudfront_origin_access_control" "site" {
  name                              = "${var.bucket_name}-oac"
  origin_access_control_origin_type = "s3"
  signing_behavior                  = "always"
  signing_protocol                  = "sigv4"
}

resource "aws_cloudfront_response_headers_policy" "security" {
  name = "${replace(var.bucket_name, ".", "-")}-security-headers"

  security_headers_config {
    content_security_policy {
      content_security_policy = var.content_security_policy
      override                = true
    }

    content_type_options {
      override = true
    }

    frame_options {
      frame_option = "DENY"
      override     = true
    }

    referrer_policy {
      referrer_policy = "strict-origin-when-cross-origin"
      override        = true
    }

    strict_transport_security {
      access_control_max_age_sec = 31536000
      include_subdomains         = true
      preload                    = true
      override                   = true
    }
  }
}

# AWS managed CachingOptimized cache policy.
data "aws_cloudfront_cache_policy" "optimized" {
  name = "Managed-CachingOptimized"
}

resource "aws_cloudfront_distribution" "site" {
  # checkov:skip=CKV_AWS_68:WAF is a recurring cost; static assets only, API is not behind CloudFront (docs/terraform-scan.md).
  # checkov:skip=CKV2_AWS_47:No WAF, see CKV_AWS_68.
  # checkov:skip=CKV_AWS_86:Access logging deferred for cost at dev scale.
  # checkov:skip=CKV_AWS_310:Single S3 origin; origin failover not justified.
  # checkov:skip=CKV_AWS_374:Geo restriction not a product requirement.
  # checkov:skip=CKV2_AWS_42:Default CloudFront certificate until a custom domain is introduced.
  # checkov:skip=CKV_AWS_174:Default certificate enforces TLS; minimum protocol applies once a custom certificate is used.
  enabled             = true
  is_ipv6_enabled     = true
  default_root_object = "index.html"
  price_class         = "PriceClass_100"
  comment             = "Einkaufio web (${var.bucket_name})"

  origin {
    domain_name              = aws_s3_bucket.site.bucket_regional_domain_name
    origin_id                = "s3-site"
    origin_access_control_id = aws_cloudfront_origin_access_control.site.id
  }

  default_cache_behavior {
    target_origin_id           = "s3-site"
    viewer_protocol_policy     = "redirect-to-https"
    allowed_methods            = ["GET", "HEAD"]
    cached_methods             = ["GET", "HEAD"]
    compress                   = true
    cache_policy_id            = data.aws_cloudfront_cache_policy.optimized.id
    response_headers_policy_id = aws_cloudfront_response_headers_policy.security.id
  }

  # SPA deep links: S3 returns 403 for unknown keys behind OAC.
  custom_error_response {
    error_code         = 403
    response_code      = 200
    response_page_path = "/index.html"
  }

  restrictions {
    geo_restriction {
      restriction_type = "none"
    }
  }

  viewer_certificate {
    cloudfront_default_certificate = true
  }

  tags = var.tags
}

data "aws_iam_policy_document" "site" {
  statement {
    sid       = "CloudFrontOACRead"
    actions   = ["s3:GetObject"]
    resources = ["${aws_s3_bucket.site.arn}/*"]

    principals {
      type        = "Service"
      identifiers = ["cloudfront.amazonaws.com"]
    }

    condition {
      test     = "StringEquals"
      variable = "AWS:SourceArn"
      values   = [aws_cloudfront_distribution.site.arn]
    }
  }

  statement {
    sid     = "DenyInsecureTransport"
    effect  = "Deny"
    actions = ["s3:*"]
    resources = [
      aws_s3_bucket.site.arn,
      "${aws_s3_bucket.site.arn}/*",
    ]

    principals {
      type        = "*"
      identifiers = ["*"]
    }

    condition {
      test     = "Bool"
      variable = "aws:SecureTransport"
      values   = ["false"]
    }
  }
}

resource "aws_s3_bucket_policy" "site" {
  bucket = aws_s3_bucket.site.id
  policy = data.aws_iam_policy_document.site.json

  depends_on = [aws_s3_bucket_public_access_block.site]
}
