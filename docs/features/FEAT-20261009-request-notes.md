---
feature_id: FEAT-20261009-request-notes
status: done
owner: TBD
depends_on: []
impacts: [request-storage, git-panel, request-editor]
adr: [docs/adr/ADR-20261009-request-notes-sidecar.md]
plan: docs/plans/FEAT-20261009-request-notes-plan.md
---

# FEAT-20261009-request-notes - Markdown notes per request

## Objective

Each HTTP request can carry free-form markdown notes, edited and previewed
inside the request document, stored next to the request as a plain `.md` file.

## Scope

- `notes` tab in the request meta line (`headers · body · auth · notes`).
- Edit mode: code editor with markdown highlighting.
- Preview mode: rendered markdown (links open in the browser).
- Toggle edit ⇄ preview; opens in preview when notes exist, in edit when empty.
- Same local-first save as the rest of the request (autosave 400 ms, ⌘S, send, close, quit).
- Storage: `workspaces/{ws}/req_{id}.md`; duplicate copies it, delete removes it.
- Git panel labels `req_{id}.md` as the request's notes.
- MCP: `request.get` returns `notes`; `request.create`/`request.update` accept `notes` (`""` clears).

## Non-Goals

- Notes on folders, workspaces, WebSocket or gRPC items.
- Importing/exporting descriptions from Postman/Insomnia/Yaak/OpenAPI — follow-up.
- Side-by-side live preview.

## Contracts

- Inputs: user markdown text.
- Outputs: `req_{id}.md` (UTF-8, verbatim); absent when notes are empty.
- Existing modules read: `RequestStore`, `RequestEditor` save/dirty flow.
- Existing modules changed: `yuki-storage` request store, `yuki-git::classify_path`, app editor/document/backend/git label.

## Acceptance Criteria

- [x] Typing in the notes editor writes `req_{id}.md` ~400 ms after the last keystroke; the request YAML is untouched.
- [x] Clearing the notes removes `req_{id}.md` (no empty files in git).
- [x] Reopening the request shows the notes rendered in preview.
- [x] Duplicating a request (or its folder) copies its notes; deleting it (or its folder) removes them.
- [x] Git panel shows `req_{id}.md` as `<METHOD> <name> · notes`.

## Blocking Decisions

- Storage location — decided by user: sidecar `.md` file (see ADR).

## Assumptions

- Notes are not loaded with the tree; read on demand when a tab opens.
- Notes changes don't bump the request YAML `updatedAt`.

## Impact

- Allowed write paths: `crates/yuki-storage/src/request/**`, `crates/yuki-git/src/{lib,tests}.rs`, `app/src/{editor,document,notes,backend,git,main}.rs`, `app/locales/pt-BR.yml`, `app/Cargo.toml`, `AGENTS.md` (Data section line).
- Read-only paths: `crates/yuki-core/**`, `crates/yuki-mcp/**`.
- Forbidden paths: on-disk YAML schema of requests.

## Open Questions

- None.
