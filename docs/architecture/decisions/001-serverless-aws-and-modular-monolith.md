# ADR-001: Serverless AWS and modular monolith

- Status: Accepted
- Date: 2026-11-24

## Context
Einkaufio is a small collaborative application. Idle cost and operational work should be minimized while keeping code maintainable.

## Decision
Use AWS Lambda for the Rust API, initially one Axum modular monolith on ARM64 using the Lambda custom runtime. Use a Lambda Function URL as the initial HTTPS ingress. Do not add API Gateway, containers, VPC, NAT Gateway, RDS or WebSockets in v1.

## Consequences
- Compute scales down when idle.
- One artifact and deployment surface.
- Domain modules stay separated in code.
- Function URL is public; Cognito JWT validation and application authorization are mandatory.
- Function URL lacks API Gateway usage plans and integrated throttling; revisit if API governance or abuse protection needs grow.
- Measure cold starts and runtime behavior.

## Alternatives
ECS/Fargate adds operational resources; API Gateway adds API management but also another service; one Lambda per route adds deployment and permission complexity.
