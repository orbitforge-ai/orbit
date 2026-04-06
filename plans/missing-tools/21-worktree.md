# Plan: `worktree` Tool — Git Worktree Isolation

> **Source**: Claude Code's `EnterWorktree` / `ExitWorktree` tools. Not present in OpenClaw.
> **Approach**: New tool — git isolation is a fundamentally new capability.

## Context

Claude Code's worktree tools let agents create isolated git worktrees for safe experimentation. The agent works on a separate branch in a separate directory, then can keep or discard changes. This is invaluable for agents doing risky refactors, trying multiple approaches, or working in parallel on different branches.

## What It Does

A `worktree` tool with actions: `create` (enter isolated worktree), `exit` (leave with keep/discard), `list` (show active worktrees). Creates a new branch and working directory inside `~/.orbit/agents/{id}/worktrees/`.

## Backend Changes

### New file: `src-tauri/src/executor/tools/worktree.rs`

> Implements `ToolHandler` trait. Register in `tools/mod.rs`: add `pub mod worktree;` and `Box::new(worktree::WorktreeTool)` to `all_tools()`.

**Tool definition** (returned by `definition()`):

```rust
ToolDefinition {
    name: "worktree".to_string(),
    description: "Create an isolated git worktree for safe experimentation. Work on a separate branch without affecting the main workspace. Actions: create, exit, list.".to_string(),
    input_schema: json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "enum": ["create", "exit", "list"],
                "description": "Action to perform"
            },
            "name": {
                "type": "string",
                "description": "Worktree name (for create). Auto-generated if omitted."
            },
            "base_branch": {
                "type": "string",
                "description": "Branch to base the worktree on (default: HEAD)"
            },
            "keep_changes": {
                "type": "boolean",
                "description": "For 'exit': if true, keep the worktree and branch. If false, remove everything. Default: true."
            }
        },
        "required": ["action"]
    }),
}
```

**Execution** (in `execute()` method):

```rust
"worktree" => {
    let action = input["action"].as_str().ok_or("worktree: missing action")?;

    match action {
        "create" => {
            let name = input["name"].as_str()
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("wt-{}", &uuid::Uuid::new_v4().to_string()[..8]));
            let base = input["base_branch"].as_str().unwrap_or("HEAD");

            let wt_dir = ctx.workspace_root
                .parent().unwrap()  // agent root
                .join("worktrees").join(&name);

            // git worktree add -b {branch_name} {path} {base}
            let output = tokio::process::Command::new("git")
                .args(["worktree", "add", "-b", &format!("orbit/{}", name),
                       wt_dir.to_str().unwrap(), base])
                .current_dir(&ctx.workspace_root)
                .output().await
                .map_err(|e| format!("git worktree failed: {}", e))?;

            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).to_string());
            }

            // Switch the ToolExecutionContext workspace_root to the worktree
            // (requires mutable context or a side-channel)
            Ok((json!({
                "worktree_path": wt_dir.display().to_string(),
                "branch": format!("orbit/{}", name),
                "status": "created"
            }).to_string(), false))
        }
        "exit" => {
            let keep = input["keep_changes"].as_bool().unwrap_or(true);
            // If !keep: git worktree remove {path} --force
            // If keep: just switch context back to main workspace
            Ok(("Exited worktree.".to_string(), false))
        }
        "list" => {
            let output = tokio::process::Command::new("git")
                .args(["worktree", "list", "--porcelain"])
                .current_dir(&ctx.workspace_root)
                .output().await
                .map_err(|e| format!("git worktree list failed: {}", e))?;
            Ok((String::from_utf8_lossy(&output.stdout).to_string(), false))
        }
        _ => Err(format!("worktree: unknown action '{}'", action)),
    }
}
```

### `src-tauri/src/executor/permissions.rs`

```rust
"worktree" => {
    let action = input["action"].as_str().unwrap_or("list");
    match action {
        "list" => (RiskLevel::AutoAllow, String::new()),
        "create" => (RiskLevel::Prompt, "Create git worktree".to_string()),
        "exit" => {
            let keep = input["keep_changes"].as_bool().unwrap_or(true);
            if keep {
                (RiskLevel::AutoAllow, String::new())
            } else {
                (RiskLevel::Prompt, "Remove worktree and discard changes".to_string())
            }
        }
        _ => (RiskLevel::Prompt, "Worktree action".to_string()),
    }
}
```

## Frontend Changes

### `src/components/chat/toolVisuals.ts`

```ts
import { GitBranchPlus } from 'lucide-react';
worktree: { Icon: GitBranchPlus, colorClass: 'text-emerald-400' },
```

### `src/screens/AgentInspector/ConfigTab.tsx`

Add to "Execution" category:

```ts
{ id: 'worktree', label: 'Git Worktree' },
```

## Permission Level

- `list`: **AutoAllow**
- `create`: **Prompt** (creates branch and directory)
- `exit` with keep: **AutoAllow**
- `exit` with discard: **Prompt** (destroys work)

## Dependencies

- `git` CLI (already required for Orbit workspace functionality)
- No new crates needed

## Key Design Decisions

- **Agent workspace switch**: After creating a worktree, subsequent file operations should target the worktree path. This requires the ToolExecutionContext to support workspace path switching.
- **Branch naming**: `orbit/{name}` prefix to avoid conflicts with user branches
- **Cleanup**: Worktrees auto-removed when agent session ends (configurable)
- **Requires git repo**: Only works if the workspace is inside a git repository

## Verification

1. `worktree { action: "create", name: "experiment" }` -> creates worktree + branch
2. Make file changes in worktree via write_file -> changes isolated
3. `worktree { action: "list" }` -> shows active worktrees
4. `worktree { action: "exit", keep_changes: true }` -> keeps branch, returns to main
5. `worktree { action: "exit", keep_changes: false }` -> removes everything
6. Test in non-git workspace -> clear error