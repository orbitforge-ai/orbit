# File Watch Trigger Security Note

`trigger.fs-watch` uses Tauri's filesystem plugin from the renderer so users can
pick arbitrary local files and folders for automation triggers. The default
desktop capability grants a broad `fs:scope` of `**` plus read metadata and
watch/unwatch permissions. The fs plugin also disables literal-dot matching so
hidden paths such as `~/.orbit/...` continue to work after the path-picker's
temporary runtime grant is gone.

This is an intentional v1 tradeoff for a local desktop automation app: the user
explicitly chooses paths through the OS dialog, and watchers must survive normal
workflow edits and sync from other devices. If Orbit adds a sandboxed mode, this
scope should be replaced with per-path grants tied to picker results.
