---
feature_id: FEAT-20261009-request-notes
status: done
owner: TBD
source: docs/features/FEAT-20261009-request-notes.md
adr: [docs/adr/ADR-20261009-request-notes-sidecar.md]
---

# FEAT-20261009-request-notes Implementation Plan

## Goal

Markdown notes per HTTP request: `notes` tab with edit ⇄ preview, persisted
local-first to `req_{id}.md` by `RequestStore`.

## Delivery Routing

Preset: full
Routing evidence:
- New on-disk artifact (`req_{id}.md`) in user-owned, git-tracked workspace folders → persistence trigger.
- Single workstream (store + app wiring are one vertical slice) → no waves, inline execution.
Cost profile: balanced
Freeze point: after focused verification and one post-execution review bundle

## Requirements and Constraints

- REQ-001: `RequestStore::get_notes`/`save_notes` read/write `req_{id}.md`; empty notes remove the file; unchanged notes don't rewrite it.
- REQ-002: `copy_request` (duplicate request/folder) copies notes; `delete_request` (incl. folder cascade) removes them.
- REQ-003: `classify_path("req_{id}.md")` → `(Request, id)`; git panel labels it `<METHOD> <name> · notes`.
- REQ-004: `notes` tab: markdown editor (tree-sitter-markdown highlight) and rendered preview, toggle; preview by default when non-empty.
- REQ-005: Notes edits use the request's dirty/autosave/⌘S path; request YAML untouched when only notes change.
- CON-001: No change to `HttpRequest` / request YAML schema.
- CON-002: UI copy through `t!`, pt-BR keys added (`locales_test`).
- CON-003: Colors only via theme; `crates/` stays gpui-free.

## Traceability Matrix

| REQ | Design / Task | Test (file or case) | Status |
|---|---|---|---|
| REQ-001 | Task 1 | `yuki-storage request::tests::notes_lifecycle_follows_the_request` | covered |
| REQ-002 | Task 1 | `yuki-storage request::tests::notes_lifecycle_follows_the_request` | covered |
| REQ-003 | Task 2 | `yuki-git tests` classify assertion (`req_ABC.md`) | covered |
| REQ-004 | Task 3 | visual smoke: edit, preview, reopen → preview | covered |
| REQ-005 | Task 3 | visual smoke: typed → `.md` written, YAML sha unchanged | covered |

## Decision State

Blocking decisions:
- Storage location — resolved by user 2026-10-09: sidecar `.md` (ADR-20261009-request-notes-sidecar).

Assumptions:
- Layout — resolved by user: `notes` tab with edit ⇄ preview toggle.
- Notes load on tab open (background read), not with the tree.
- Notes don't bump the YAML `updatedAt`.

Open questions:
- None.

Approval required before execution:
- Given: user asked to implement ("Quero permitir…") and answered both decisions.

## Ownership

Allowed write paths:
- `crates/yuki-storage/src/request/**`
- `crates/yuki-git/src/lib.rs`, `crates/yuki-git/src/tests.rs`
- `app/src/{editor,document,notes,backend,git,main}.rs`, `app/Cargo.toml`, `app/locales/pt-BR.yml`
- `AGENTS.md` (Data/Layout lines only)

Read-only paths:
- `crates/yuki-core/**`, `crates/yuki-mcp/**`

## Libraries

| Library | Version | Doc ref | Why |
|---|---|---|---|
| gpui-kit feature `tree-sitter-markdown` | 0.7 (already a dep) | `gpui-component-0.7.1/Cargo.toml` feature list | markdown highlight in the editor |
| gpui-base `text::TextView::markdown` | 0.7 (already a dep) | `gpui-base-0.7.1/src/text/text_view.rs:213` | rendered preview; links open via `App::open_url` |

## Tasks

Execution: single workstream, inline by the Coordinator.

### Task 1 - Store owns the sidecar

Files:
- Modify: `crates/yuki-storage/src/request/requests.rs`, `crates/yuki-storage/src/request/mod.rs` (test)

Steps:
- [ ] `notes_path`, `get_notes` (missing → ""), `save_notes` (empty → remove, same → no-op, else `write_atomic`).
- [ ] `copy_request` copies the source `.md` when present; `delete_request` removes it.
- [ ] One lifecycle test: save/get, unchanged no-op, duplicate copies, folder duplicate copies, clear removes, delete removes.

Verification:
- Command: `cargo test -p yuki-storage notes`
- Expected result: test passes.

Stop conditions:
- `write_atomic` unsuitable for `.md` (it isn't YAML-specific — check).

### Task 2 - Git classification

Files:
- Modify: `crates/yuki-git/src/lib.rs`, `crates/yuki-git/src/tests.rs`, `app/src/git.rs`

Steps:
- [ ] `classify_path` accepts `req_{id}.md` as Request.
- [ ] `git_label` appends ` · notes` for `.md` paths.

Verification:
- Command: `cargo test -p yuki-git classify`
- Expected result: passes with the new `req_ABC.md` assertion.

### Task 3 - Notes tab

Files:
- Create: `app/src/notes.rs` (state helpers + render)
- Modify: `app/src/editor.rs` (state, dirty, save), `app/src/document.rs` (meta segment + pane), `app/src/backend.rs`, `app/src/main.rs` (mod), `app/Cargo.toml` (feature), `app/locales/pt-BR.yml`

Steps:
- [ ] `Backend::notes` / `save_notes`.
- [ ] `RequestEditor`: `notes: Entity<EditorState>` (markdown), `notes_saved`, `notes_preview`; async load on open; `ReqTab::Notes`.
- [ ] Dirty = request draft ≠ saved OR notes ≠ notes_saved; save writes request (if changed) and notes in one background task.
- [ ] Meta segment `notes` / `notes —`; pane with edit/preview toggle; preview via `TextView::markdown(...).scrollable(true)`.

Verification:
- Command: `cargo build -p yuki && cargo test -p yuki`
- Expected result: builds; locale tests pass.
- Smoke: type notes → `req_{id}.md` written, YAML unchanged; reopen → preview; duplicate/delete; clear → file gone.

## Validation

status: clean

| # | Check | Result | Note |
|---|---|---|---|
| V-001 | Every REQ maps to at least one Task | pass | |
| V-002 | Every Task has Verification (command + expected) | pass | |
| V-003 | No blocking decision left open (Decision Gate) | pass | storage decided by user |
| V-004 | Parallel workstreams have non-overlapping write paths | n-a | single workstream |
| V-005 | Every Worker row in Launch Spec maps to one Task | n-a | inline |
| V-006 | Traceability Matrix has no `gap` row | pass | |
| V-007 | Every new/version-pinned library has a `Libraries` row with a doc ref | pass | |

## Final Verification

- Command: `cargo build -p yuki && cargo test -p yuki -p yuki-storage -p yuki-git -p yuki-mcp && cargo clippy -p yuki -p yuki-storage -p yuki-git`
- Expected result: green; visual smoke of the notes tab.

## Risks

- RISK-001: Notes typed before the async load finishes could be overwritten — load applies only if the editor is still untouched.
- RISK-002: An external tool deleting `req_{id}.yaml` leaves an orphan `.md` (invisible to the tree; harmless).

## Post-execute Updates

| # | Severity | Area | Finding | Decision | Status |
|---|---|---|---|---|---|
| 1 | P1 | notes tab | Empty tab flipped to preview on the first keystroke (mode derived from content), unmounting the editor mid-typing. | Preview default decided once at load; `notes_preview` starts `false`. | applied |
| 2 | P3 | cleanup | `match bool` in `notes_label` / `git_label`. | Rewritten as `if`. | applied |
| 3 | P2 | MCP | Notes invisible to agents (`request.get` read only the YAML). | `request.get` returns `notes`; `request.create`/`request.update` take `notes` (`""` clears) via `RequestStore::get_notes`/`save_notes`. Test: `yuki-mcp api::tests::request_notes_round_trip`. | applied |

Evidence (2026-10-09): `cargo test -p yuki -p yuki-storage -p yuki-git -p yuki-mcp` green; clippy clean; smoke on `/tmp/yk`: typed notes → `req_ucrskb2t.md` written, YAML sha unchanged; preview renders heading/bold/link/list/code; ⌘W + reopen → preview; sidebar duplicate → copy `.md`; delete copy → both files gone; ⌘A ⌫ → `.md` removed.
Not exercised: git panel label (pure format; classification unit-tested); folder duplicate/cascade in UI (covered by store test).
