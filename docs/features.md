# Features

<!-- Insert each row ordered by ID (date), not appended at a fixed bottom — keeps the index merge-safe across parallel worktrees. -->

| ID | Title | Status | Owner | Depends on | Impacted areas | Docs |
|---|---|---|---|---|---|---|
| FEAT-20261009-request-notes | Markdown notes per request (editor + preview) | done | TBD | - | `crates/yuki-storage/src/request/**`, `crates/yuki-git/src/lib.rs`, `app/src/{editor,document,notes,backend,git}.rs` | [brief](features/FEAT-20261009-request-notes.md), [plan](plans/FEAT-20261009-request-notes-plan.md), [adr](adr/ADR-20261009-request-notes-sidecar.md) |
| FEAT-20261013-import-export | Import/export Voleeo, Yaak, Postman, Insomnia | done | TBD | - | `crates/yuki-import/**`, `crates/yuki-export/**`, `app/src/{transfer,transfer_ui,import_view,import_render,export_view,workspace,palette,chrome,main}.rs` | [brief](features/FEAT-20261013-import-export.md), [plan](plans/FEAT-20261013-import-export-plan.md), [adr](adr/ADR-20261013-import-export-boundary.md) |
| FEAT-20261014-template-functions | Template functions for fake values (`$fake.*`, `$uuid.v4`, `$timestamp.*`) | done | TBD | - | `crates/yuki-mcp/src/resolve/**`, `app/src/{vars,complete}.rs` | [brief](features/FEAT-20261014-template-functions.md), [plan](plans/FEAT-20261014-template-functions-plan.md) |

## Status Values

- `draft`
- `blocked`
- `planned`
- `approved`
- `in_progress`
- `done`
- `deprecated`
