#!/usr/bin/env bash
set -euo pipefail
input=$(cat)
if ! command -v jq >/dev/null 2>&1; then
  echo 'jq is required for the Claude Code commit hook' >&2
  exit 1
fi
[[ "$(jq -r '.tool_name // empty' <<< "$input")" == Bash ]] || exit 0
cmd=$(jq -r '.tool_input.command // empty' <<< "$input")
if grep -Eq '(^|[[:space:];&|])git([[:space:]]+-C[[:space:]]+[^[:space:]]+)?[[:space:]]+commit([[:space:]]|$)' <<< "$cmd"; then
  cwd=$(jq -r '.cwd // empty' <<< "$input")
  cd "${cwd:-${CLAUDE_PROJECT_DIR:-$PWD}}" || exit 1
  cd "$(git rev-parse --show-toplevel)" || exit 1
  make check
fi
