---
feature_id: FEAT-20261014-template-functions
status: done
owner: TBD
source: docs/features/FEAT-20261014-template-functions.md
adr: []
---

# FEAT-20261014-template-functions Implementation Plan

## Goal

No-arg template functions (`$fake.*`, `$uuid.v4`, `$timestamp.*`) evaluated by the
shared resolver, plus editor chip/autocomplete awareness.

## Delivery Routing

Preset: standard
Routing evidence:
- Syntax already exists on disk (Voleeo `{{ ns.fn() }}`); no schema/persistence change → no ADR.
- One vertical slice (resolver + two app helpers) → inline, no waves.
Cost profile: balanced

## Requirements and Constraints

- REQ-001: `resolve_str` replaces `{{ <known fn>() }}` with a generated value; per occurrence; also inside variable values (transitive).
- REQ-002: Unknown functions and calls with arguments stay verbatim.
- REQ-003: `$fake.cpf()`/`$fake.cnpj()` produce valid check digits (formatted).
- REQ-004: Editor chips: known fn = Function, unknown fn = Missing; autocomplete lists functions.
- CON-001: No new third-party crates (`rand`, `uuid`, `chrono` are workspace deps).
- CON-002: `crates/` stays gpui-free; UI copy via `t!` + pt-BR.

## Traceability Matrix

| REQ | Task | Test | Status |
|---|---|---|---|
| REQ-001 | Task 1 | `yuki-mcp resolve::vars::tests::functions_evaluate_per_occurrence` | covered |
| REQ-002 | Task 1 | `yuki-mcp resolve::vars::tests::leaves_unknown_function_verbatim` | covered |
| REQ-003 | Task 1 | `yuki-mcp resolve::funcs::tests::cpf_cnpj_check_digits` | covered |
| REQ-004 | Task 2 | `yuki vars::tests` kind assertions + visual smoke (autocomplete) | covered |

## Decision State

Blocking decisions:
- Naming and repetition — resolved by user 2026-10-14 (see brief).

Assumptions:
- Generators are a port of `field-faker/src/lib/fake.js` (`firstOption` dropped: browser-only).
- Autocomplete: functions after variables, same `MAX_ITEMS` cap.

Approval required before execution:
- Given: user asked to add the feature and answered both decisions.

## Ownership

Allowed write paths: see brief → Impact.

## Tasks

Execution: single workstream, inline by the Coordinator.

### Task 1 - Resolver evaluates functions

Files: create `crates/yuki-mcp/src/resolve/funcs.rs`; modify `resolve/{mod,vars}.rs`, `crates/yuki-mcp/Cargo.toml`.

Steps:
- [ ] `funcs::NAMES` table (`name`, generator) and `funcs::call(expr) -> Option<String>` (accepts `name()` only).
- [ ] `resolve_inner`: non-identifier → `funcs::call` or verbatim.
- [ ] Tests: cpf/cnpj check digits; resolve fn, fresh per occurrence, via var, unknown/args verbatim.

Verification: `cargo test -p yuki-mcp resolve` → green.

### Task 2 - Editor awareness

Files: `app/src/vars.rs`, `app/src/complete.rs`, `app/locales/pt-BR.yml`.

Steps:
- [ ] `vars::kind`: non-identifier → `Function` if `funcs::is_known`, else `Missing`.
- [ ] `suggestions`: append matching function names (`fake.email()`); popup shows `function` as origin.

Verification: `cargo test -p yuki` → green; smoke.

## Validation

status: clean

| # | Check | Result | Note |
|---|---|---|---|
| V-001 | Every REQ maps to a Task | pass | |
| V-002 | Every Task has Verification | pass | |
| V-003 | No blocking decision open | pass | user answered |
| V-004 | Parallel write paths disjoint | n-a | single workstream |
| V-005 | Launch Spec rows map to Tasks | n-a | inline |
| V-006 | Traceability has no gap | pass | |
| V-007 | New libraries have doc ref | n-a | workspace deps only |

## Final Verification

- `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p yuki -p yuki-mcp`
- Smoke on `/tmp/yk`: send with `fake.*` in body → timeline shows generated values.

## Risks

- RISK-001: The variables panel/tooltip preview of a var holding a function shows a different value on each refresh — acceptable (it's a sample).

## Post-execute Updates

| # | Severity | Area | Finding | Decision | Status |
|---|---|---|---|---|---|
| 1 | P3 | tests | `funcs::lookup` test duplicated `leaves_unknown_function_verbatim`. | Dropped; `{{ fake.email }}` (no parens) case moved into the resolver test. | applied |
| 2 | P3 | autocomplete | Function suggestions rendered blue like variables. | Magenta, matching the chip. | applied |
| 3 | P1 | syntax | User asked for a standard prefix: functions and variables looked alike. | `$` sigil required (`funcs::lookup` strips it; autocomplete inserts `$name()`; `$` is a name char for the `{{` filter). Unprefixed Voleeo `uuid.v4()` now verbatim. | applied |

Evidence (2026-10-14, after the `$` revision): `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings` clean; `cargo test -p yuki -p yuki-mcp` green; doc-parity in sync. Smoke on `/tmp/yk` (Local env, `FAKE_ID: '{{ $fake.uuid() }}'`): chips magenta for `$fake.*`/`$timestamp.iso()`, red for unprefixed `uuid.v4()` (`1 missing`); ⌘↵ → timeline `GET …?email=larissa.barbosa79@example.com&cpf=96.773.437/9434-49&ts=2026-10-14T15:40:09.468Z&v=fd00f78c-…&old={{ uuid.v4() }}` (CNPJ check digits verified by hand); typing `{{ $fake.ci` → popup `$fake.city()` · `function`, ↵ saved `{{ $fake.city() }}` to the YAML.
Not exercised: body/headers/auth fields (same `resolve_str` path, covered by URL); MCP `request.send` (same `send_http`).
