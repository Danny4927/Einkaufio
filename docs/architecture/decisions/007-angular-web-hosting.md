# ADR-007: Angular web hosting on S3 and CloudFront

- Status: Accepted
- Date: 2026-11-24

## Context
Angular production output is static for v1.

## Decision
Store assets in a private S3 bucket and deliver them through CloudFront using Origin Access Control. Configure HTTPS and SPA route fallback. Keep API origin separate.

## Consequences
- No web server or always-on compute.
- Cache hashed assets long-term and keep index.html short-lived.
- Configure CORS for the web origin.
- Custom domain/certificate may add cost; use CloudFront hostname initially if desired.
- This serves only the web client, not Android.

## Alternatives
Serving frontend from Lambda wastes compute; public S3 website hosting is less suitable for private-origin HTTPS delivery.
