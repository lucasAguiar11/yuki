---
feature_id: FEAT-20261014-template-functions
status: done
owner: TBD
depends_on: []
impacts: [template-resolution, request-editor, mcp-send]
adr: []
plan: docs/plans/FEAT-20261014-template-functions-plan.md
---

# FEAT-20261014-template-functions - Template functions for fake values

## Objective

`{{ $fake.email() }}`, `{{ $fake.cpf() }}`, `{{ $uuid.v4() }}`, `{{ $timestamp.iso() }}`, …
resolve to generated values at send time, in every field that takes templates.
Generators ported from `field-faker/src/lib/fake.js` (Brazilian-flavoured data).

## Scope

- `$fake.*`: fullName, firstName, lastName, email, phone, cpf, cnpj, uuid, lorem,
  number, password, date, username, company, street, city, zipCode, jobTitle, url.
- `$uuid.v4`, `$timestamp.iso|unix|unixMs|date|time`. Voleeo's unprefixed
  `uuid.v4()` stays verbatim (one syntax).
- Evaluated in `yuki_mcp::resolve::resolve_str` → app send, MCP `request.send`,
  WebSocket connect, gRPC, OAuth "get token" all get it.
- Each occurrence draws a fresh value (decided by user).
- Editor: known functions chip magenta ("evaluated at send time"); unknown ones
  red ("not defined"); `{{` autocomplete lists functions after variables.

## Non-Goals

- Arguments (`uuid.v3(name=…)`, `password(length=…)`) — tokens with args stay verbatim.
- Response chaining (`response.body(...)`), `op.read`, plugins.
- Cookie values (`yuki-cookies` has its own resolver).

## Contracts

- Syntax: Voleeo's `{{ ns.fn() }}` (already chipped by `app/src/vars.rs`); saved text stays literal.
- Unknown function tokens pass through verbatim (as today) — `encrypt(value=…)` chips keep working.

## Acceptance Criteria

- [x] Sending with `{{ $fake.email() }}` / `{{ $fake.cpf() }}` sends a generated email and a CPF with valid check digits; timeline shows the values.
- [x] An env var whose value is `{{ $fake.uuid() }}` resolves through the variable.
- [x] `{{ nope.fn() }}`, `{{ uuid.v4() }}` and `{{ uuid.v3(name="a") }}` are sent verbatim.
- [x] Typing `{{ $fa` lists `$fake.*` functions; ↵ inserts `{{ $fake.email() }}` as a chip.

## Blocking Decisions

- Naming — decided by user: `$` sigil (Postman-style) + namespaces `fake`/`uuid`/`timestamp`; first round picked unprefixed names, revised the same day before commit.
- Repetition — decided by user: fresh value per occurrence.

## Impact

- Allowed write paths: `crates/yuki-mcp/{Cargo.toml,src/resolve/**}`, `app/src/{vars,complete}.rs`, `app/locales/pt-BR.yml`, `AGENTS.md`, `README.md`, `README.pt-BR.md`, `Cargo.lock`.
- Read-only: `crates/yuki-cookies/**`, on-disk request YAML schema.
