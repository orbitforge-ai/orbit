# ~~Plan: `process` Tool~~

> **MERGED INTO PLAN 03**: This tool has been reconciled into the expanded `shell_command` tool (plan 03). See [03-exec-background.md](03-exec-background.md).

Claude Code doesn't have a separate Process tool — it handles background process management through the same Bash tool via `run_in_background` + follow-up calls. Orbit should follow the same pattern: expand `shell_command` with `process_action` parameter instead of creating a separate tool.