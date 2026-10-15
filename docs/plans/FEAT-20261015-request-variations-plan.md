---
feature_id: FEAT-20261015-request-variations
status: done
owner: TBD
source: docs/features/FEAT-20261015-request-variations.md
adr: [ADR-20261015-request-variations-inline]
---

# FEAT-20261015-request-variations Implementation Plan

## Goal

Named variations per HTTP request with whole-field overrides (url, params,
headers, body, auth), the active one persisted in the request YAML, applied
once in `send_http` for app and MCP, edited in place in the request editor.

## Delivery Routing

Preset: full
Routing evidence:
- On-disk user data schema change (`req_{id}.yaml`) → ADR.
- MCP public contract change (`request.send` gains `variation`).
- Secrets handling extended (seal/open/mask of variation auth).
- One vertical slice over shared types → single workstream, sequential tasks,
  no waves (Orchestration Gate: sequential).
Cost profile: balanced
Freeze point: after focused verification and one post-execution review bundle
Baseline (pre-edit, 2026-10-15): `cargo test --workspace` → all green, 0 failed.

## Requirements and Constraints

- REQ-001: `Variation { id, name, url?, parameters?, headers?, body?, auth? }`; `HttpRequest.variations` / `active_variation` serde-default, omitted when empty/none; `Some(vec![])` survives a round trip.
- REQ-002: `HttpRequest::effective(pick)`: explicit id-or-name > `active_variation` > base; `Some` fields replace whole; result has no variations/active; unknown explicit pick → error; dangling active → base.
- REQ-003: `send_http` applies `effective` after both request loads (`response_sources`, `send_once`) and in `resolve_http` (no-op when already effective); history/snapshots hold the effective request only.
- REQ-004: MCP `request.send` accepts optional `variation`; `request.get`/`list` mask variation auth secrets (and restore on masked round trip where `auth_to_store` applies).
- REQ-005: Storage: `copy_request` (request + folder duplicate) copies variations; a whole-request save path writes them through `save_request_if_changed` (one write, no-op keeps `updated_at`).
- REQ-006: App load opens and app save seals every auth (`auths_mut`), reusing stored ciphertext for an unchanged variation secret (matched by variation id).
- REQ-007: Editor fold: with a variation active, each of the five fields is an override iff its edited value differs from the base; base fields on disk never change; with none active, edits go to the base.
- REQ-008: Variation chip in the meta line: list/switch (persisted), `+ new variation…`, rename, delete; overridden segments marked; `use base` drops one override.
- REQ-009: Import/export: Voleeo bundle carries variations with secret marking/stripping on variation auth; import flattening of inherited auth/headers applies to variation overrides too; `convert.rs` builds empty variations.
- REQ-010: cURL (tab and no-tab fallback), OAuth "get token" and meta-line counts use the effective request.
- CON-001: No new crates. `crates/` stays gpui-free.
- CON-002: UI copy via `t!` + `app/locales/pt-BR.yml`; colors via theme.
- CON-003: Files without variations byte-identical after open + save (AGENTS.md: on-disk data belongs to the user).

## Release Invariants

1. Editing while a variation is active never changes base fields on disk.
2. A request YAML without variations re-serializes byte-identically.
3. Variation auth secrets are ciphertext at rest in encrypted workspaces, masked in MCP reads, and keep their ciphertext when unchanged.
4. Responses/snapshots never contain `variations`/`activeVariation`.
5. App ⌘↵ and MCP `request.send` (no `variation`) send the same effective request.

## Traceability Matrix

| REQ | Task | Test | Status |
|---|---|---|---|
| REQ-001 | Task 1 | `yuki-core types::http::variation_tests::empty_override_survives_round_trip_and_none_is_omitted` | covered |
| REQ-002 | Task 1 | `yuki-core types::http::variation_tests::effective_overlays_whole_fields_and_drops_variations` | covered |
| REQ-003 | Task 3 | MCP smoke (stdio bridge): explicit/none/id/unknown variation; stored responses hold no `variations` | covered |
| REQ-004 | Task 3 | `yuki-mcp api::tests::variation_secrets_are_masked_and_unknown_variation_fails_send` | covered |
| REQ-005 | Task 2 | `yuki-storage request::tests::put_request_saves_variations_keeps_identity_and_skips_noop` | covered |
| REQ-006 | Task 5 | `yuki secrets::tests::seal_request_keeps_variation_ciphertext_matched_by_id` | covered |
| REQ-007 | Task 5 | `yuki draft::tests::fold_writes_only_differences_into_the_active_variation` | covered |
| REQ-008 | Task 5 | app smoke: create, override, switch, use base, rename, delete (confirm), persisted active on open | covered |
| REQ-009 | Task 4 | `yuki-export tests/roundtrip.rs::voleeo_bundle_round_trips_fully` + `tests/secrets.rs` with a variation | covered |
| REQ-010 | Task 5 | cURL/no-tab fallback go through `resolve_http` → `shown()`; OAuth/meta use `edited` (code path; cURL click not exercised) | covered |

## Decision State

Blocking decisions: resolved by user 2026-10-15 (see brief) — whole fields; selection in YAML; per request.

Assumptions:
- `method` and `name` are not overridable (a different method is a different request).
- `url_override` (MCP) still beats a variation URL.
- Switching variation saves the tab first, then rebuilds the editor (no in-place reload API; rebuilding avoids leaked subscriptions and stale `AuthForm`/`PathParams` caches).
- New variation starts with no overrides (= base) and is activated.
- Notes stay per request.

Open questions:
- None.

Approval required before execution:
- User approval of this plan.

## Ownership

Allowed write paths: see brief `Impact`.
Read-only: `crates/yuki-storage/src/{response,snapshot}.rs`.
Forbidden: WS/gRPC types and UI.

## Dependencies

| Dependency | Type | Notes |
|---|---|---|
| ADR-20261009-request-notes-sidecar | prior decision | Related storage choice; not superseded (notes stay a sidecar). |
| ADR-20261013-import-export-boundary | prior decision | Flattening of inherited auth/headers on import extends to variation overrides. |

## Tasks

Execution: single workstream, sequential, one bounded Worker (or inline) + one Validator on the frozen diff.

### Task 1 - Core type and overlay

Files: modify `crates/yuki-core/src/types/http.rs`; struct-literal sites listed in the scout map (`yuki-import/src/convert.rs:108`, test literals in `yuki-storage`, `yuki-http`, `yuki-mcp`, `yuki-export/tests`).

Steps:
- [ ] `Variation` (+ `Type, Serialize, Deserialize, Debug, Clone, PartialEq`, camelCase, every override `Option` with `skip_serializing_if = "Option::is_none"`).
- [ ] `variations`, `active_variation` on `HttpRequest`.
- [ ] `effective(&self, pick: Option<&str>) -> Result<HttpRequest>`, `auths_mut()` (base + `Some` variation auths).
- [ ] Fix every struct literal.

Verification: `cargo test -p yuki-core` → new tests green; `cargo build --workspace` compiles.

### Task 2 - Storage

Files: `crates/yuki-storage/src/request/{requests,mod}.rs`, `folders.rs`.

Steps:
- [ ] `copy_request` literal copies `variations`/`active_variation`; folder copy too.
- [ ] `put_request(ws, &HttpRequest)`: keeps disk identity (`workspace_id`, `folder_id`, `order`, `created_at`), writes the rest via `save_request_if_changed`.

Verification: `cargo test -p yuki-storage request` → green incl. new tests.

### Task 3 - Send pipeline and MCP

Files: `crates/yuki-mcp/src/api/request/{send,crud}.rs`, `api/redact.rs`, `api/tools/request.rs`, `api/server.rs`, `resolve/http.rs`, `api/mod.rs` tests.

Steps:
- [ ] `send_http(.., variation: Option<&str>)`; overlay after `get_request` in `response_sources` and `send_once`; recursion passes `None` (sources use their own active).
- [ ] `resolve_http` applies `effective(None)`.
- [ ] `request.send` schema + handler `variation`; error on unknown; instructions hint.
- [ ] Masking in `request_list`/`request_get` via `auths_mut`; `request.update` description: edits base only.

Verification: `cargo test -p yuki-mcp` → green; MCP smoke on `/tmp/yk` with a local server (base vs `variation`, `response.get` has no `variations`).

Stop conditions: overlay cannot be applied before `inherit` without changing auth inheritance semantics → stop and report.

### Task 4 - Import/export

Files: `crates/yuki-import/src/voleeo.rs`, `crates/yuki-export/src/{lib,voleeo}.rs`, `crates/yuki-export/tests/{roundtrip,secrets}.rs`, `app/src/transfer.rs` (seal on import).

Steps:
- [ ] Secret marking/stripping and import sealing walk `auths_mut`.
- [ ] Inherit flattening applies to variation `auth`/`headers` overrides.

Verification: `cargo test -p yuki-import -p yuki-export` → green incl. extended cases.

### Task 5 - App

Files: create `app/src/variation.rs` (chip + menu + switch/new/rename/delete/use-base); modify `app/src/{editor,document,draft,backend,secrets,curl,oauth,items,workspace}.rs`, `app/locales/pt-BR.yml`.

Steps:
- [ ] `draft::fold(saved, edited) -> HttpRequest` (pure): no active → base = edited; active → overrides = per-field diff vs base. `RequestEditor::draft()` returns the folded, disk-shaped request.
- [ ] `RequestEditor::new` loads widgets from `saved.effective(None)` (normalized per `query.rs`); `effective(cx)` accessor for cURL, OAuth, meta counts, `AuthHost::stored_auth`.
- [ ] `Backend::load` opens / `save` seals all auths (variation matched by id); `save` uses `put_request`; `items.rs:122` keeps `saved` disk-shaped.
- [ ] Switch / new / delete / `use base`: save, mutate `saved`, persist, rebuild the tab's editor (`workspace.rs` replaces `tabs[ix]`).
- [ ] Chip leftmost in `render_meta` (always shown: `base ▾`), overridden segment mark, right-click menus; strings + pt-BR.

Verification: `cargo test -p yuki` → green incl. `draft::tests::fold_*`, `secrets` test, `locales_test`; app smoke per acceptance criteria (skill `yuki-visual-smoke`).

Stop conditions: rebuilding the editor loses the response panel or focus in a way the existing `focus_active` can't restore → report before working around.

## Validation

status: clean

| # | Check | Result | Note |
|---|---|---|---|
| V-001 | Every REQ maps to a Task | pass | |
| V-002 | Every Task has Verification | pass | |
| V-003 | No blocking decision open | pass | 3 decided by user |
| V-004 | Parallel write paths disjoint | n-a | single workstream |
| V-005 | Launch Spec rows map to Tasks | n-a | sequential |
| V-006 | Traceability has no gap | pass | |
| V-007 | New libraries have doc ref | n-a | none |

## Final Verification

- `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
- `.agents/skills/yuki-i18n/scripts/doc-parity.sh`
- App + MCP smoke per brief acceptance criteria.

## Risks

- RISK-001: Legacy `?query` in a variation URL: `query::normalize` mints fresh param ids; diffing against an un-normalized base yields a spurious override. Mitigation: normalize base and each URL override in memory at load (no write until edited).
- RISK-002: A tab open while MCP/git changes the file overwrites it on next save (existing behavior; more visible now that switching writes).
- RISK-003: `headers` override freezes the list (ADR NEG-004); `use base` is the escape hatch.
- RISK-004: Pre-existing: MCP `request.update`/`request.create` return base auth unmasked. Out of scope; separate fix proposal.
- RISK-005: `app/src/editor.rs`/`workspace.rs` already exceed 250 lines; new UI goes to `variation.rs`, not into them.

## Review Findings

| # | Severity | Area | Finding | Decision | Status |
|---|---|---|---|---|---|
| 1 | P1 | app | `draft()` feeds six writers; returning the effective request would write variation values into the base. | `draft()` returns the folded disk-shaped request; readers needing effective use `effective(cx)`. | applied |
| 2 | P1 | engine | `response_sources` loads the request without `resolve_request`; overlaying only there would miss sources. | Overlay right after both `get_request` calls. | applied |
| 3 | P1 | security | `prefill_responses`/history walk the whole serialized request; inactive variations (and their secrets) would leak into history/snapshots. | `effective` clears `variations`/`active_variation`. | applied |
| 4 | P2 | storage | `update_request` takes 8 explicit args and cannot write variations. | New `put_request` whole-struct save. | applied |

## Post-execute Updates

| # | Severity | Area | Finding | Decision | Status |
|---|---|---|---|---|---|
| 1 | P1 | app | Validator: switching variation during an in-flight send/save — the old completion's `mark_saved` restored the old `saved` under the rebuilt form; the autosave then folded the new form over the old variation (base overwritten or override dropped). | `change()` returns while `sending \|\| saving`; the chip is disabled meanwhile. Smoke: slow endpoint, chip during ⌘↵ doesn't open, file unchanged after the send. | applied |
| 2 | P2 | app | Rebuild during a send dropped its cancel/elapsed state. | Same guard. | applied |
| 3 | P3 | app | RISK-001 mitigation missing: a variation URL with `?query` would be rewritten on open. | `RequestEditor::new` normalizes each URL override in memory (written only when edited). | applied |
| 4 | P3 | engine | `send_http` doc lost its `$response.body` pointer paragraph. | Restored. | applied |
| 5 | P3 | docs | Brief said rename/delete/use base on right-click; built in the chip's menu (one place). | Brief updated to the menu. | applied |
| 6 | P3 | security | MCP `request.update`/`duplicate` echo the request; variation secrets would leak through them. | `mask_variations` on their output; `request.list`/`get` mask base + variations. Base auth on update/create stays as before (RISK-004). | applied |
| 7 | P3 | i18n | `use base: %{field}` shows the English field key in pt-BR. | Kept: the meta line shows the same English labels (`headers`, `auth`). | rejected |
| 8 | P3 | scope | One-line test literal edits in `yuki-storage/{response,snapshot}.rs` (read-only in the plan) and `app/src/params.rs` (`saved`→`shown`). | Required by the new struct fields / the shown form. | applied |

Evidence (2026-10-15): `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -D warnings` clean; `cargo test --workspace` → 465 passed, 0 failed (baseline 0 failed); `git diff --check` clean on feature files; doc-parity in sync. App smoke on an isolated data dir with a local echo server: new variation → auth override TOKEN_B (base TOKEN_A untouched on disk) → ⌘↵ sent TOKEN_B; base → TOKEN_A and `activeVariation` removed; `use base: auth` → override removed, sent TOKEN_A; rename → name on disk; delete via confirm → `variations`/`activeVariation` gone; persisted active variation shown on open; untouched request files byte-identical. MCP smoke (`yuki mcp`): `request.get` masks variation token (reveal shows it); `variation` by name/id beats the active one; none → active; unknown → `not found: variation nope`; stored responses hold no `variations`.
Not exercised: encrypted workspace end-to-end (unit test of `seal_request` only), Voleeo import flattening of variation overrides, copy-as-cURL click with a variation active.

### Revision 2026-10-15 — export

| # | Severity | Area | Finding | Decision | Status |
|---|---|---|---|---|---|
| 9 | P2 | export | User: in Postman/Insomnia/Yaak exports, each variation becomes a request (was: skipped with a warning). | `yuki_export::variations_as_requests` expands every bundle at the top of `to_postman_with_roots`/`to_insomnia`/`to_yaak`: base (no variations) then `{request} · {variation}` = `effective(variation)`, id `{request}-{variation}`, same `order` (stable sort keeps it right after the base). Skip warning removed. Test: `roundtrip.rs::http_formats_export_each_variation_as_a_request_after_its_base` (all three formats re-imported). | applied |
| 10 | P1 | semantics | User: editing one request changed every variation (inheritance: a field a variation didn't override followed the base). Decided: a variation is an independent copy. | `Variation::copy_of` on create (copies the screen); `draft::fold` writes all five fields into the active variation; `HttpRequest::detach_variations` fills older partial variations on load (memory only, no write until edited); `use base` and the blue override marks removed. Test `draft::tests::variations_are_independent_copies` (replaces the diff-based fold test, contract changed by user). Smoke: create `sa` → full copy on disk; edit body in `sa` → only `sa`; switch to base, edit body → base only, `sa` keeps its body. | applied |
