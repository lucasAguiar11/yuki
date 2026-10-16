---
feature_id: FEAT-20261016-paste-curl
status: done
owner: TBD
source: docs/features/FEAT-20261016-paste-curl.md
adr: []
---

# FEAT-20261016-paste-curl Implementation Plan

## Goal

A cURL pasted into the URL bar becomes the open request.

## Delivery Routing

Preset: standard (Level 0 weight)
Routing evidence:
- No persistence/contract change; `change()` already writes and rebuilds the form → no ADR.
- One vertical slice → inline, no waves.
Cost profile: balanced

## Requirements and Constraints

- REQ-001: `curl::parse` maps method, URL, query, headers, raw/form/multipart body, basic auth; skips unknown flags; handles `\` line continuations and quotes.
- REQ-002: Paste of `curl …` in the URL bar applies it via `change`; other pastes unchanged.
- CON-001: `shlex` (MIT/Apache, already in the lockfile) for shell splitting.

## Traceability Matrix

| REQ | Task | Test | Status |
|---|---|---|---|
| REQ-001 | Task 1 | `yuki paste_curl::tests::*` (DevTools sample, also newline-stripped) | covered |
| REQ-002 | Task 1 | visual smoke | covered |

## Decision State

Blocking decisions: none.
Assumptions:
- Replaces url/params/headers/body/auth of the shown request; name/folder untouched.
- `-d` without `-X` → POST (curl's rule); `-G` moves data into the query.

## Tasks

Execution: single workstream, inline by the Coordinator.

### Task 1 - Parse and apply

Files: `app/src/paste_curl.rs` (new; `curl.rs` was already 219 lines), `app/src/{editor,main}.rs`, `app/Cargo.toml`.

Steps:
- [x] `curl::parse(&str) -> Option<Parsed>` + `apply(&mut HttpRequest)`.
- [x] URL `InputEvent::Change` paste branch: `curl ` prefix → `change(apply)`.
- [x] Tests: round-trip of `curl()` output; DevTools sample.

Verification: `cargo test -p yuki paste_curl` → green; smoke.

## Validation

status: clean

| # | Check | Result | Note |
|---|---|---|---|
| V-001 | Every REQ maps to a Task | pass | |
| V-002 | Every Task has Verification | pass | |
| V-003 | No blocking decision open | pass | |
| V-004 | Parallel write paths disjoint | n-a | single workstream |
| V-006 | Traceability has no gap | pass | |
| V-007 | New libraries have doc ref | pass | shlex already transitive |

## Final Verification

- `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p yuki`
- Smoke on `/tmp/yk`: paste a DevTools cURL → method/params/headers/body populated, saved YAML matches.

## Risks

- RISK-001: Whole-request overwrite on paste; undo is the git panel / history restore.

## Post-execute Updates

| # | Severity | Area | Finding | Decision | Status |
|---|---|---|---|---|---|
| 1 | P2 | input | The single-line URL input strips line breaks before the paste reaches us, leaving `\` + spaces between args. | Harmless (escaped space token is a skipped positional); test covers the stripped form. | applied |

Evidence (2026-10-16): `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings` clean; `cargo test -p yuki` 36 passed; doc-parity in sync. Smoke on `/tmp/yk`: pasted a 5-line DevTools-style cURL into "Copy of /photos" → POST `http://127.0.0.1:8765/post`, `params 2 · headers 2 · body json`; saved YAML has `page`/`q` params, both headers, `body: {kind: json, text: '{"name":"yuki"}'}`, `auth: none`.
Not exercised: paste while a variation is active (same `change` path as the variation menu), basic auth/multipart in the UI (unit-tested).
