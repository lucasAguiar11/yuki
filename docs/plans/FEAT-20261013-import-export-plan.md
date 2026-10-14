---
feature_id: FEAT-20261013-import-export
status: done
owner: TBD
source: docs/features/FEAT-20261013-import-export.md
adr: [docs/adr/ADR-20261013-import-export-boundary.md]
---

# FEAT-20261013-import-export Implementation Plan

## Goal

Import Voleeo bundles, Yaak, Postman and Insomnia (v4+v5) files into a new or
the current workspace with preview/selection; export a workspace to Voleeo,
Yaak, Postman and Insomnia v4, optionally without secrets.

## Delivery Routing

Preset: full
Routing evidence:
- Writes user-owned workspace/env files and seals secrets (persistence + security).
- External file formats are public contracts (other tools must read our exports).
- Two engine crates with disjoint write scopes can be built in parallel against a fixed contract; the app slice depends on both.
Cost profile: balanced
Freeze point: after Wave 2 focused verification and one review bundle

## Execution Profile

Model preference: inherit
Override scope: workers
Reasoning effort: inherit
Cost profile: balanced
Verification profile: focused-waves, frozen-diff-final

## Pre-edit Baseline (2026-10-13)

- `cargo test -p yuki-import -p yuki-export`: 52 passed, 0 failed (import 45, export 7).
- `cargo test -p yuki`: 22 passed.

## Requirements and Constraints

- REQ-001: `detect_format` recognizes Voleeo bundles (`voleeoBundle` key) and Insomnia v5 (`type: collection.insomnia.rest/5.0`); `parse` reads Insomnia v5 YAML into the IR (requests, folders, base env + sub-environments, auth, headers, body).
- REQ-002: `plan(format, content, selected, workspace_id, order_base)` returns a `ConvertedPlan` for every format incl. Voleeo, with fresh ids, `workspace_id` set and parent links remapped; `preview` works for Voleeo too.
- REQ-003: Flattening: each request carries the effective auth (nearest non-inherit up the chain: request → folders → collection/workspace) and merged headers (collection/workspace → folders → request; a nearer header replaces a farther one with the same name, case-insensitive). Folders come out with `AuthConfig::None`, no headers.
- REQ-004: Named environments become `Environment`s in the plan (fresh ids, `shared: false`; Voleeo keeps `shared`, colors, secret flags); globals stay in `variables`.
- REQ-005: Voleeo bundle import keeps WebSocket and gRPC items (fresh ids, remapped folder links); several bundled workspaces → one top-level folder each; workspace DNS overrides dropped with a warning.
- REQ-006: `to_yaak` writes a `yaakSchema: 4` JSON that `yuki_import` reads back (folders, requests, envs, auth, body); `to_insomnia` writes Insomnia v4 JSON likewise.
- REQ-007: `Bundle::strip_secrets` blanks every secret: auth secret fields (bearer token, basic password, api key value, oauth secrets/tokens, digest/ntlm passwords, aws secret/session) at workspace/folder/request/ws/grpc level, and values of variables flagged `encrypted`.
- REQ-008: App import: file picker → detect (unknown → error message) → screen with tree checkboxes, counts, warnings, destination (new workspace named after the collection / current workspace root or a folder) → persist through stores; encrypted destination seals auth and secret vars; then opens the workspace/refreshes the tree.
- REQ-009: App export: format choice (Voleeo, Yaak, Postman, Insomnia) + include-secrets toggle (off) → save dialog → file written (Postman envs as sibling files); warnings in the status line.
- REQ-010: Entry points: palette "Import collection…", "Export workspace…"; welcome screen button; ⌘O "+ import collection…".
- CON-001: `crates/` stay gpui-free; crypto stays in the app (`secrets.rs`).
- CON-002: No change to on-disk YAML schemas; every import goes through existing stores.
- CON-003: UI copy via `t!` + pt-BR keys; colors via theme; files ≤ ~250 lines.
- CON-004: Protected baseline tests stay green; edits to `tests/convert.rs` only where REQ-003 changes the asserted contract (folder auth/headers now flattened).

## Traceability Matrix

| REQ | Design / Task | Test (file or case) | Status |
|---|---|---|---|
| REQ-001 | Task 1 | `yuki-import tests/insomnia_v5.rs` (fixture v5 → IR counts, auth, envs) + detect cases | covered |
| REQ-002 | Task 1 | `yuki-import tests/plan.rs` ids fresh/remapped, selection honored, Voleeo preview | covered |
| REQ-003 | Task 1 | `tests/plan.rs` flatten: folder bearer → request; header override order | covered |
| REQ-004 | Task 1 | `tests/plan.rs` Insomnia/Yaak sub-envs → `environments` | covered |
| REQ-005 | Task 1 | `tests/plan.rs` bundle with ws+grpc+2 workspaces | covered |
| REQ-006 | Task 2 | `yuki-export tests/yaak.rs`, `tests/insomnia.rs` round-trip via `yuki_import::parse` | covered |
| REQ-007 | Task 2 | `yuki-export tests/secrets.rs` every secret kind blanked, non-secrets kept | covered |
| REQ-008 | Task 3 | smoke: import 4 fixtures, both destinations, selection, encrypted dest `enc:v1` | covered |
| REQ-009 | Task 3 | smoke: export 4 formats with/without secrets; re-import each | covered |
| REQ-010 | Task 3 | smoke: palette, welcome, ⌘O entries | covered |

## Decision State

Blocking decisions (resolved by user 2026-10-13):
- Export secrets: ask per export, off by default.
- Inheritance: flatten on import.
- Destination: new or current workspace.
- Selection: preview tree with checkboxes.
- Insomnia: read v4+v5, export v4.

Assumptions:
- Multi-workspace sources → one top-level folder per workspace.
- Imported named envs local; Voleeo envs keep `shared`.
- Fresh ids on import.
- Export scope: the whole current workspace.

Open questions:
- None.

Approval required before execution:
- Given: user asked to add the feature and answered the five decisions.

## Ownership

Allowed write paths:
- Task 1: `crates/yuki-import/**`
- Task 2: `crates/yuki-export/**`
- Task 3: `app/src/**`, `app/Cargo.toml`, `Cargo.lock`, `app/locales/pt-BR.yml`, `README.md`, `README.pt-BR.md`, `AGENTS.md`

Read-only paths:
- `crates/yuki-core/**`, `crates/yuki-storage/**`, `crates/yuki-crypto/**`, `crates/yuki-mcp/**`

## Contract (fixed before Wave 1)

```rust
// yuki-import
pub enum ImportFormat { OpenApi, Swagger2, Postman, Insomnia, Bruno, Yaak, Voleeo } // Insomnia = v4 | v5
pub fn detect_format(content: &str) -> Option<ImportFormat>;
pub fn preview(format: ImportFormat, content: &str) -> Result<ImportPreview, ImportError>;
pub fn plan(format: ImportFormat, content: &str, selected: Option<&[String]>,
            workspace_id: &str, order_base: f64) -> Result<ConvertedPlan, ImportError>;
pub struct ConvertedPlan {
    pub folders: Vec<ApiFolder>,          // flattened: auth None, headers empty
    pub requests: Vec<HttpRequest>,       // effective auth + merged headers
    pub variables: Vec<EnvironmentVariable>, // → Global env
    pub environments: Vec<Environment>,   // named envs, fresh ids, workspace_id set
    pub websockets: Vec<WsConnection>,    // Voleeo only
    pub grpc: Vec<GrpcRequest>,           // Voleeo only
    pub warnings: Vec<String>,
}
// root_auth is removed from ConvertedPlan (flattened into requests).

// yuki-export
pub fn to_yaak(bundles: &[Bundle]) -> Result<ExportResult, YukiError>;
pub fn to_insomnia(bundles: &[Bundle]) -> Result<ExportResult, YukiError>;
impl Bundle { pub fn strip_secrets(&mut self); }
// existing: to_voleeo, to_postman, postman_environments
```

## Parallelization

| Workstream | Task | Write scope | Depends on |
|---|---|---|---|
| WS-A import engine | Task 1 | `crates/yuki-import/**` | contract |
| WS-B export engine | Task 2 | `crates/yuki-export/**` | contract (tests use existing `yuki_import::parse` only) |
| WS-C app | Task 3 | `app/**`, docs | WS-A, WS-B |

## Wave Schedule

| Wave | Workstreams | Gate to advance |
|---|---|---|
| 1 | WS-A, WS-B (parallel) | `cargo test -p yuki-import -p yuki-export` green; clippy clean on both |
| 2 | WS-C (Coordinator inline: UI needs visual smoke) | `cargo test -p yuki`, clippy, smoke |
| 3 | Validators per workstream on the frozen diff | validated |

## Subagent Launch Spec

| Agent | Role | Task | model_tier | Writes | Reads | Stop conditions |
|---|---|---|---|---|---|---|
| ImportEngine | Worker | Task 1 | standard | `crates/yuki-import/**` | core, export (read) | contract can't hold; a baseline test must change beyond REQ-003 |
| ExportEngine | Worker | Task 2 | standard | `crates/yuki-export/**` | core, import (read) | format doc contradicts fixture shape |
| ValidateA / ValidateB | Validator | Task 1 / Task 2 | fast | none | frozen diff | — |

## Tasks

### Task 1 - Import engine (WS-A)

Files: `crates/yuki-import/src/{lib,detect,convert,insomnia}.rs`, new `src/insomnia_v5.rs`, `src/voleeo.rs`, tests `tests/{insomnia_v5,plan}.rs`, fixtures `tests/fixtures/insomnia/export_v5.yaml`, `tests/fixtures/voleeo/bundle.yaml`.

Steps:
- [ ] Insomnia v5: detect + parse (collection tree, `environments` base + `subEnvironments`, auth, headers, body, parameters) per the v5.1 schema; fixture mirrors a real v5 export.
- [ ] Voleeo: detect `voleeoBundle`; preview tree from the bundle; plan maps core types directly (fresh ids, remap `folder_id`/`workspace_id`, envs/ws/grpc kept).
- [ ] `ConvertedPlan` per contract; `plan()` = parse/select/build or bundle path; flattening per REQ-003 for all formats; named envs per REQ-004.
- [ ] Update `tests/convert.rs` only where folder auth/headers are now flattened (REQ-003).

Verification:
- Command: `cargo test -p yuki-import && cargo clippy -p yuki-import --all-targets -- -D warnings`
- Expected: all baseline tests + new ones green.

### Task 2 - Export engine (WS-B)

Files: `crates/yuki-export/src/{lib,yaak,insomnia}.rs` (new modules), `src/auth.rs` if shared helpers, tests `tests/{yaak,insomnia,secrets}.rs`.

Steps:
- [ ] `to_yaak`: `yaakSchema: 4` JSON mirroring `crates/yuki-import/tests/fixtures/yaak/export.json` (workspaces, folders, httpRequests, environments; base env `parentModel: workspace`).
- [ ] `to_insomnia`: v4 JSON (`_type: export`, `__export_format: 4`, `resources[]` with `workspace`, `request_group`, `request`, `environment` base + subs) mirroring `crates/yuki-import/tests/fixtures/insomnia/export.json`.
- [ ] Both: templates normalized as each tool expects; unsupported items (ws/grpc) → warnings.
- [ ] `Bundle::strip_secrets` per REQ-007.

Verification:
- Command: `cargo test -p yuki-export && cargo clippy -p yuki-export --all-targets -- -D warnings`
- Expected: baseline 7 + new round-trip/secret tests green.

### Task 3 - App import/export (WS-C)

Files: new `app/src/transfer.rs` (Backend: persist plan, load+open bundle), `app/src/import_view.rs` (screen), `app/src/export.rs` (dialog + write); modify `main.rs` (actions), `workspace.rs`/`palette.rs`/`chrome.rs` (entries, center screen), `app/Cargo.toml` (deps), `pt-BR.yml`, READMEs, `AGENTS.md`.

Steps:
- [ ] Persist: dest new → `workspaces.create`; existing → reparent roots under the chosen folder, `order_base` after siblings; `secrets::key` → seal auth/vars when encrypted; `write_bulk`; Global env merge (append missing keys); named envs `save`; ws/grpc `save`.
- [ ] Export load: workspace + folders + requests + envs + ws + grpc with secrets opened → `Bundle`; strip unless toggled; serialize; `prompt_for_new_path`; Postman env siblings.
- [ ] Import screen (center screen pattern like settings): tree with checkboxes, counts, warnings, destination select, confirm/cancel.
- [ ] Entries per REQ-010; copy + pt-BR.

Verification:
- Command: `cargo build -p yuki && cargo test -p yuki && cargo clippy -p yuki --all-targets -- -D warnings`
- Smoke: per REQ-008/009/010 on `/tmp/yk`, with the user's own Yuki window moved/minimized first.

## Validation

status: clean

| # | Check | Result | Note |
|---|---|---|---|
| V-001 | Every REQ maps to at least one Task | pass | |
| V-002 | Every Task has Verification (command + expected) | pass | |
| V-003 | No blocking decision left open | pass | five decisions answered |
| V-004 | Parallel workstreams have non-overlapping write paths | pass | yuki-import vs yuki-export |
| V-005 | Every Worker row in Launch Spec maps to one Task | pass | |
| V-006 | Traceability Matrix has no `gap` row | pass | |
| V-007 | New/version-pinned libraries have a doc ref | n-a | no new crates (app gains path deps only) |

## Final Verification

- Command: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
- Expected: green; smoke evidence recorded under Post-execute Updates.

## Risks

- RISK-001: Our exporters are only proven against our own parsers; fixtures copied from real exports reduce the gap, but a real Yaak/Insomnia/Postman import is a manual check for the user.
- RISK-002: Flattening loses inheritance (ADR NEG-001).
- RISK-003: Yaak's current schema may have moved past 4 (parser fixture is 2024.1); Yaak imports older schemas.
- RISK-004: Large collections: persistence is one `write_bulk`; preview tree for thousands of rows is a long list (acceptable).

## Wave Execution Log

| Wave | Workstream | Status | Evidence |
|---|---|---|---|
| 1 | WS-A import engine | done | `cargo test -p yuki-import` 56 passed (45 baseline + 11); clippy clean; no baseline assertion changed |
| 1 | WS-B export engine | done | `cargo test -p yuki-export` 14 passed (7 baseline + 7); clippy clean |
| 2 | WS-C app (inline) | done | `cargo test -p yuki` 22 passed; clippy clean; smoke below |
| 3 | Validators | validated | WS-A validated; WS-B and WS-C refuted once (P1s below), revalidated after fixes |

## Post-execute Updates

| # | Severity | Area | Finding | Decision | Status |
|---|---|---|---|---|---|
| 1 | P1 | app import | Folder variables flagged secret (Voleeo bundles) were written in plaintext into encrypted workspaces. | `seal_vars` on every plan folder inside the key block. | applied |
| 2 | P1 | app export | Postman env sibling files could overwrite each other or existing files. | `transfer::write_export`: `<stem>.<env>.postman_environment.json`, `-2`, `-3`… when taken. | applied |
| 3 | P1 | yuki-export | Yaak/Insomnia exports dropped `Content-Type` (Yuki adds it at send time), so bodies went out untyped. | `implied_content_type` (same mimes as yuki-http) unless a header is already in scope; tests assert present/absent. | applied |
| 4 | P1 | yuki-import | Insomnia v5 WebSocket/gRPC items became bogus `GET` requests. | v5 requires `method` (or `req_` id); others skipped with one warning; test added. | applied |
| 5 | P2 | app | Opening settings/environments while an import/export screen is open leaves the transfer screen on top (focused screen hidden). | Follow-up: close `transfer` in `open_settings`/`open_envs`. | deferred → follow-up |
| 6 | P2 | yuki-import | A disabled nearer header removes an enabled farther one with the same name while flattening; v5 ignores `meta.sortKey`. | Follow-up. | deferred → follow-up |
| 7 | P2 | yuki-export | Yaak: AWS/OAuth1/Digest/NTLM exported as no auth (Yaak has plugins); OAuth2 audience/redirect/state/PKCE not written. Insomnia: disabled vars exported as active, NTLM workstation dropped. `body.content_type` applied to every body kind. | Warnings added for each (acceptance "never silently"); real mappings are follow-up. | partially applied → follow-up |
| 8 | P2 | app | Import isn't atomic (new workspace created before parse; partial writes on a late error); `order_base` ignores ws/grpc siblings; export double-click opens two save dialogs; `import_render.rs` 272 lines. | Follow-up. | deferred → follow-up |

Evidence (2026-10-13, frozen diff + P1/warning fixes): `cargo fmt --all --check` ok; `cargo clippy --workspace --all-targets -- -D warnings` 0 errors; `cargo test --workspace` 428 passed / 0 failed; `git diff --check` clean; doc parity ok. WS-B and WS-C revalidated after the P1 fixes.
Throwaway end-to-end (removed): Postman 2.1, Insomnia v4, Insomnia v5, Yaak and Voleeo fixtures imported through `Backend::import` (0 requests left with `Inherit`), each exported to Voleeo/Yaak/Postman/Insomnia with and without secrets and re-imported with the same request count; secrets-off exports had empty bearer/basic secrets; encrypted destination stored `enc:v1:` bearer tokens and sealed folder vars; folder reparenting and single-request selection held; Postman env siblings numbered on a second export.
Smoke (`/tmp/yk`): welcome link → file dialog → import screen (insomnia 5, tree, unchecked one request, destination "into demo" → `/JSONPlaceholder`) → 2 requests under the chosen folder with flattened bearer, status "2 requests importados"; ⌘⇧P "Exportar workspace…" → yaak → save dialog → `demo.yaak.json` (yaakSchema 4), status "exportado para …".
Not exercised: opening our exports in the real Yaak/Insomnia/Postman apps (RISK-001, user check); a send of an imported request (flattening verified on stored data).
Checkpoint: 1 garbage=clean · 2 copies(selectable chips: settings language, export format, import destination)=3/3 → TRIGGERED — proposal: `ui::chip` shared helper, migrate the three (own change) · 3 hub(`render_center`)=5/6 · 4 real-data=n/a
