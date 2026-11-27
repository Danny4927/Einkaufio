---
name: deliver
description: Autonomous delivery workflow - plan, implement with tiered subagents (Haiku / Opus low / Opus high escalation), verify, review, apply feedback and open a pull request. Use when the user invokes /deliver <task>.
---
# /deliver – tiered autonomous delivery

Input: the task description in the arguments. You are the orchestrator: delegate work to the subagents below, keep only summaries in your context, and decide on escalation. Never accept a subagent's claim of success without the real command output.

## Agents and routing

| Tier / role | Agent | Model |
|---|---|---|
| simple | `scout` | Haiku |
| standard | `implementer` | Opus, low effort |
| hard or escalation | `implementer-deep` | Opus, high effort |
| planning | `planner` | Opus, low effort |
| review | `reviewer` | Opus, high effort |

## Workflow

1. **Prepare.** Work on the session's designated branch (create it from the default branch if needed). Run `git status`; stop if there are unrelated uncommitted changes.
2. **Plan.** Call `planner` with the task. If it returns `QUESTIONS:` that change the outcome, ask the user (AskUserQuestion) before continuing. Track steps with the task list.
3. **Implement each step in order**, passing the step and relevant prior results:
   - `simple` → `scout`; `standard` → `implementer`; `hard` → `implementer-deep`.
   - Escalation: if `scout` reports `STATUS: BLOCKED`, retry with `implementer`. If `implementer` reports `STATUS: BLOCKED`, or its verify target is red, retry **once** with `implementer-deep`, including the failure report. If `implementer-deep` is blocked, stop and report to the user with the options it gave.
   - Run independent steps in parallel only if they touch disjoint files.
4. **Verify.** Run `make check`, then `make verify` yourself. On failure, send the failing output to `implementer` (escalating as above). At most 2 fix rounds; then stop and report.
5. **Review.** Run in parallel: the `reviewer` agent, and the `/code-review medium` and `/security-review` skills on the diff.
6. **Apply feedback.** Fix every confirmed `blocker`/`major` finding via the routing above (security/concurrency findings go to `implementer-deep`). Decide on `minor` findings and note skipped ones for the PR. Re-run `make verify`. At most 2 review rounds; if `reviewer` still says `CHANGES_REQUESTED` on blockers, stop and report.
7. **Docs.** Ensure `docs/roadmap.md` and affected OpenAPI/ADR/security/quality docs are updated (delegate mechanical edits to `scout`).
8. **Commit and push.** Small, meaningful commits with the required attribution trailer; `git push -u origin <branch>`. Never force-push.
9. **Pull request.** Create it with the GitHub MCP tools, filling `.github/pull_request_template.md`. List the checks that ran with their real result, checks that could not run, skipped minor findings and any escalations. Then ask the user whether to watch the PR; if yes, `subscribe_pr_activity` and handle CI/review feedback with the same routing.

## Hard rules

- Never skip, disable or weaken tests to get green. Never claim unobserved success.
- Respect `.claude/settings.json` deny rules; never run `terraform apply`/`destroy` or AWS commands.
- Cost control: no more than one escalation per step and the round limits above; report instead of looping.
