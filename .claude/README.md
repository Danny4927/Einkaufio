# Claude Code guardrails

The deny rules in settings.json prevent Claude Code from invoking Terraform apply/destroy/import and from reading common local secret/credential files.

The optional commit hook script is a small, auditable helper. Configure it as a PreToolUse hook for Bash in the Claude Code settings used by your environment. It runs make check before a git commit command. It is not activated merely by being present in this repository; verify hook configuration in the actual Claude Code runtime.

Never put credentials in repository settings. Use environment/secret managers and review permission prompts.
