---
feature_id: FEAT-20261014-response-vars
status: done
owner: TBD
source: docs/features/FEAT-20261014-response-vars.md
adr: []
---

# FEAT-20261014-response-vars Implementation Plan

## Goal

`$response.body(requestId, selector)` resolved from stored responses in the
shared pipeline, plus "response field → variable" and its list in the app.

## Delivery Routing

Preset: standard
Routing evidence:
- No schema change: the formula lives in an ordinary env variable → no ADR.
- One vertical slice (engine function + app UI) → inline, no waves.
Cost profile: balanced
Baseline (pre-edit, 2026-10-14): `cargo test -p yuki -p yuki-mcp` → yuki 24 passed, yuki-mcp 79 + 1 passed, 0 failed.

## Requirements and Constraints

- REQ-001: `funcs::call` parses `name(key="v", …)` (escaped quotes); bad syntax → None.
- REQ-002: `resolve::prefill_responses` finds `$response.body(…)` in texts, evaluates against the latest stored response + JSONPath, and stores the result under the expression; `resolve_str` uses it. No response / no match → absent → verbatim.
- REQ-003: `send_http` prefills from env/folder vars and every string of the request; the app preview (`Backend::variables`) prefills from var values.
- REQ-004: `json_path_at(pretty_json, offset)` returns the JSONPath of the scalar/key under the caret (`$.a[0].b`, bracket-quoted keys when not identifiers).
- REQ-005: Response strip `→ variable` writes `NAME = {{ $response.body(requestId="<id>", selector="<path>") }}` to the active env (global when none) and refreshes vars; lists vars reading this request with ✕ (delete).
- REQ-006: A defined var whose value still holds an unresolved `$response.` call shows as Missing (red).
- CON-001: No new crates (`serde_json_path` is already a workspace dep).
- CON-002: UI copy via `t!` + pt-BR; colors via theme.

## Traceability Matrix

| REQ | Task | Test | Status |
|---|---|---|---|
| REQ-001 | Task 1 | `yuki-mcp resolve::funcs::tests::call_parses_args` | covered |
| REQ-002 | Task 1 | `yuki-mcp resolve::responses::tests::select_by_json_path` + MCP smoke (no response, no match) | covered |
| REQ-003 | Task 1 | smoke: send with `{{ ACCESS_TOKEN }}` (MCP + app) | covered |
| REQ-004 | Task 2 | `yuki capture::tests::path_at_caret` | covered |
| REQ-005 | Task 2 | smoke: ⌥↵, env file, readers list | covered |
| REQ-006 | Task 2 | `yuki vars::tests::kind_and_used` | covered |

## Decision State

Blocking decisions: resolved by user 2026-10-14 (see brief).

Assumptions:
- Response lookup is by request id (stable across renames).
- Matching several nodes yields a JSON array (Voleeo behavior).

## Tasks

Execution: single workstream, inline by the Coordinator.

### Task 1 - Engine

Files: `crates/yuki-mcp/src/resolve/{funcs,vars,mod,responses}.rs`, `crates/yuki-mcp/src/api/request/send.rs`, `crates/yuki-mcp/Cargo.toml`.

Verification: `cargo test -p yuki-mcp resolve` → green.

### Task 2 - App

Files: `app/src/{capture,response,editor,backend,vars,main}.rs`, `app/locales/pt-BR.yml`.

Verification: `cargo test -p yuki` → green; smoke on `/tmp/yk` with a local JSON server.

## Validation

status: clean

| # | Check | Result | Note |
|---|---|---|---|
| V-001 | Every REQ maps to a Task | pass | |
| V-002 | Every Task has Verification | pass | |
| V-003 | No blocking decision open | pass | |
| V-004 | Parallel write paths disjoint | n-a | single workstream |
| V-005 | Launch Spec rows map to Tasks | n-a | inline |
| V-006 | Traceability has no gap | pass | |
| V-007 | New libraries have doc ref | n-a | none |

## Final Verification

- `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p yuki -p yuki-mcp`

## Risks

- RISK-001: Large (windowed) bodies are read from their side file on every send that uses them — fine for tokens; slow for MB-sized bodies.
- RISK-002: The preview re-reads stored responses when vars reload; cost is one file read per referenced request.

## Post-execute Updates

| # | Severity | Area | Finding | Decision | Status |
|---|---|---|---|---|---|
| 1 | P1 | UI | User: first version looked bad — `→ variable` in the status strip, a form row with outlined input + buttons, no preview of the value. | Bar under the strip follows the caret (`path = value`), names in place as `{{ NAME }}` with `↵ save · esc cancel` hints, readers on the right; ⌥↵ binding (`ResponseBody` context). | applied |
| 2 | P1 | UI | Name input didn't take focus (keys went to the response editor). | Focus deferred to after the row renders (`window.defer`). | applied |
| 3 | P2 | UI | Caret at the top suggested `$` (whole body). | Root path ignored; the hint shows instead. | applied |
| 4 | P3 | files | `capture.rs` passed 250 lines. | Split: pure `capture.rs`, UI `capture_view.rs`. | applied |

Evidence (2026-10-14): fmt/clippy (`--workspace -D warnings`) clean; `cargo test -p yuki -p yuki-mcp` → 26 + 81 + 1 passed; doc-parity in sync. MCP smoke on `/tmp/yk` (local server): `{{ ACCESS_TOKEN }}` sent `{{ $response.body(…) }}` verbatim before any login, `xyz789` then `tok-3` after each login, verbatim again with a non-matching selector. App smoke (pt-BR): hint on open; caret on `"tok-3"` → `$.data.access_token = tok-3`; ⌥↵ → `{{ ACCESS_TOKEN }}` focused; ↵ → env file holds the formula, bar lists `ACCESS_TOKEN ← $.data.access_token`; esc cancels; `use token` chip blue, ⌘↵ sent `?t=tok-3`.
Not exercised: ✕ removal click (same `write_var` path as save, with `None`); folder-level variables; windowed (large) bodies.

### Revision 2026-10-14 — pointer (re-send sources)

| # | Severity | Area | Finding | Decision | Status |
|---|---|---|---|---|---|
| 5 | P1 | engine | User: the variable must work as a pointer, always re-sending the source. | `send_http` → `send_chain`: sends every `resolve::response_sources` (request fields + used vars, transitive) first; a source already in the chain is read from history. Test: `resolve::responses::tests::sources_follow_used_vars_only`. | applied |
| 6 | P2 | app | A tab whose request was sent as a source kept its old response; a pending var showed red although the send now resolves it. | `VarsChanged` also runs `sync_history` on every tab; a pending `$response` var is `Kind::Function`. | applied |

Evidence: MCP smoke with only the consumer sent — never-logged-in → `?t=tok-3` (login sent 1×); token changed to `tok-4` → `?t=tok-4` (2×); cycle login⇄use → `?t=tok-4`, login sent with `?u=true` from history (3×, no loop). `cargo test -p yuki -p yuki-mcp` → 26 + 82 + 1 passed; clippy clean.
