#!/usr/bin/env bash
# Optional Claude Code PreToolUse hook for git commit commands.
set -euo pipefail

command="${CLAUDE_TOOL_INPUT:-}"
if [[ "$command" == *"git commit"* ]]; then
  repo_root="$(git rev-parse --show-toplevel)"
  cd "$repo_root"
  make check
fi
