# ADR 012: Agent workflow and enforced quality gates

- Status: Accepted
- Date: 2026-11-25

## Context

The repository uses AI coding agents alongside human review. Agents need a concise source of repository instructions and a reproducible local command that matches CI. Prose-only policies are insufficient for critical security and persistence invariants.

## Decision

- Keep repository-wide instructions in root AGENTS.md; link to canonical architecture, security, quality and roadmap documents instead of duplicating them.
- Provide make verify as the local entry point mirroring the CI jobs, with narrower targets for fast feedback.
- Use a PR template to require acceptance criteria, verification evidence, security review and roadmap updates.
- Provide Claude Code project deny rules and register the commit-check helper as a Bash `PreToolUse` hook in project settings. The hook parses the documented JSON stdin payload and runs `make check` before matching commit commands. Runtime loading still depends on the environment trusting/loading project settings; verify with `/hooks` and `/status`. Permission rules and hooks are defense-in-depth, not an OS sandbox.
- Introduce typed authenticated principals and repository contract tests incrementally at the actual trust/persistence boundaries. Do not use marker types that can be constructed from unverified client input or claim adapter semantics that have no adapter implementation yet.

## Consequences

The first three controls improve consistency immediately. Strong repository contract tests will be added with the concrete adapter, while shared reusable cases should be exercised by both the in-memory fake and each production adapter. Required status checks and branch protection remain GitHub repository settings and must be configured separately from workflow YAML.
