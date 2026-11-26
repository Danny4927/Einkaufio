# Claude Code guardrails

These permission rules are defense-in-depth, not a sandbox. They do not prevent equivalent shell commands, subprocesses, external tools or connected integrations. A Read deny rule does not protect a secret from Bash(cat ...). Keep secrets outside the repository and use OS/container isolation plus least-privilege credentials.

The project settings register the PreToolUse Bash hook. Claude Code sends JSON on stdin; the hook must parse tool_input.command with jq and run make check for git commit commands. Hook timeout is configured to 600 seconds. Verify it with /hooks and /status in the actual runtime and perform a real test commit in a disposable worktree. The repository script test is not an end-to-end Claude Code runtime test.

The hook requires Bash, jq, Git and Make. Runtime project settings may not apply in all hosted/cloud sessions.
