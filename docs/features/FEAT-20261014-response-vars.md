---
feature_id: FEAT-20261014-response-vars
status: done
owner: TBD
depends_on: [FEAT-20261014-template-functions]
impacts: [template-resolution, response-view, environments, mcp-send]
adr: []
plan: docs/plans/FEAT-20261014-response-vars-plan.md
---

# FEAT-20261014-response-vars - Response fields as variables (chaining)

## Objective

Pick a field in a JSON response (e.g. `access_token` of a login) and name it
`TOKEN`; from then on `{{ TOKEN }}` in any request sends that field from the
**latest** response of the source request.

## Scope

- Template function `{{ $response.body(requestId="<id>", selector="$.path") }}`:
  latest stored response of `<id>`, through JSONPath (empty selector = whole
  body). String → raw; other JSON → its text; several matches → JSON array.
- Evaluated by the shared send pipeline (`ApiBackend::send_http`) → app and MCP;
  also in the variable preview (chips, ⌘⇧E panel), which only reads history.
- Pointer: a send that reads `$response.body` (directly or through the variables
  it uses) sends each source first; a source already up the chain (cycle) is
  read from its stored response. Source transport errors fail the send.
- Response body (JSON): a bar under the strip follows the caret
  (`$.data.token = abc`); `→ variable` (⌥↵) names the field in place
  (`{{ NAME }}`, ↵ save · esc cancel; suggested name = last key, upper snake case)
  and writes `NAME = {{ $response.body(…) }}` as a plain variable in the active
  environment (global when none).
- The same bar lists the variables that read this request
  (`TOKEN ← $.access_token`) with ✕ to remove them.

## Non-Goals

- `$response.header`, XPath, TTL/refresh strategies (always re-sends; Voleeo has the others).
- WebSocket/gRPC sends (HTTP only).

## Contracts

- On disk: only an ordinary env variable whose value is the formula (no new
  request/env schema). The token itself is never written to an environment.
- Syntax follows the `$` function sigil; arg names follow Voleeo (`requestId`, `selector`).

## Acceptance Criteria

- [x] Login returns `{"data":{"access_token":"…"}}`; caret on the value → ⌥↵ → name `ACCESS_TOKEN` → env gets the formula.
- [x] Another request with `{{ ACCESS_TOKEN }}` sends the latest token; after login returns a new one, it sends the new one.
- [x] Before any login response, the variable shows as a function (magenta) and the first send that uses it sends the login first.
- [x] The login's response bar shows `ACCESS_TOKEN ← $.data.access_token`. ✕ removes the variable — same `write_var` path as save, click not exercised in the smoke.

## Blocking Decisions

- Re-send — revised by user 2026-10-14: a pointer. Every send that reads `$response.body` (directly or via variables) sends the source first; cycles fall back to the stored response. Supersedes "missing response → visible failure".
- Mechanism — decided by user: variable holding a response function (formula), not captured values.
- Destination — decided by user: active environment, plain variable.
- Selection — decided by user: from the response + list on the source request.

## Impact

- Allowed write paths: `crates/yuki-mcp/{Cargo.toml,src/resolve/**,src/api/request/send.rs}`, `app/src/{capture,response,editor,backend,vars,var_view,main}.rs`, `app/locales/pt-BR.yml`, `README*.md`, `AGENTS.md`, `Cargo.lock`, docs.
- Read-only: `crates/yuki-storage/**` (uses `ResponseStore::latest`/`read_full_body`).
