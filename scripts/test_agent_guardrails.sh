#!/usr/bin/env bash
set -euo pipefail
repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"
python3 -m json.tool .claude/settings.json >/dev/null
bash -n .claude/hooks/pre-commit-check-stdin.sh
if command -v shellcheck >/dev/null 2>&1; then
  shellcheck .claude/hooks/pre-commit-check-stdin.sh scripts/test_agent_guardrails.sh
fi
command -v jq >/dev/null || { echo 'jq is required to test the Claude hook' >&2; exit 1; }

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/repo" "$scratch/bin"
git -C "$scratch/repo" init -q
cat >"$scratch/bin/make" <<'MAKE'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"$HOOK_TEST_LOG"
MAKE
chmod +x "$scratch/bin/make"
export HOOK_TEST_LOG="$scratch/make.log"
export PATH="$scratch/bin:$PATH"
hook="$repo_root/.claude/hooks/pre-commit-check-stdin.sh"

printf '{"tool_name":"Bash","cwd":"%s","tool_input":{"command":"git commit -m test"}}\n' "$scratch/repo" | bash "$hook"
grep -qx 'check' "$HOOK_TEST_LOG"
: >"$HOOK_TEST_LOG"
printf '{"tool_name":"Bash","cwd":"%s","tool_input":{"command":"git -C . commit -m test"}}\n' "$scratch/repo" | bash "$hook"
grep -qx 'check' "$HOOK_TEST_LOG"
: >"$HOOK_TEST_LOG"
printf '{"tool_name":"Bash","cwd":"%s","tool_input":{"command":"git status"}}\n' "$scratch/repo" | bash "$hook"
[[ ! -s "$HOOK_TEST_LOG" ]]
printf 'Claude Code hook tests passed.\n'
