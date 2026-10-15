---
title: "ADR-20261015-request-variations-inline: Request variations live inline in req_{id}.yaml as whole-field overrides"
status: "Accepted"
date: "2026-10-15"
authors: "user + agent"
feature_id: FEAT-20261015-request-variations
scope: "persistence"
tags: ["storage", "on-disk-format", "git", "request", "send-pipeline", "mcp"]
supersedes: ""
superseded_by: ""
---

# ADR-20261015-request-variations-inline

## Status

Accepted

## Context

Users duplicate a request to run it with different credentials or payloads.
Environments are one global axis (dev/prod); multiplying them per credential
explodes (dev×admin, dev×user, …). Request files belong to the user and are
committed to git. `ADR-20261009-request-notes-sidecar` kept the request schema
unchanged by moving notes to a sidecar; that reason (plain markdown readable
outside Yuki) doesn't apply to structured overrides.

## Decision

- `HttpRequest` gains `variations: Vec<Variation>` and
  `active_variation: Option<String>`, both serde-`default` and skipped when
  empty/none, inside `req_{id}.yaml`.
- `Variation { id, name, url?, parameters?, headers?, body?, auth? }`: an
  independent copy — the app writes all five (`Variation::copy_of`) and fills
  a partial one from the base on load (`detach_variations`, in memory), so a
  base edit never reaches a variation. `None` (hand-written/older) falls back
  to the base at send time. `Some(vec![])` (no headers) is distinct from `None`.
- `HttpRequest::effective(pick)` in `yuki-core` is the single overlay:
  explicit pick (id or name) > `active_variation` > base; returns a request
  with `variations` cleared and `active_variation = None`.
- `ApiBackend::send_http` applies it right after loading the request (both
  the source-chain pass and the send), so app and MCP share it and history /
  snapshots never contain inactive variations.
- The app keeps its in-memory `saved` disk-shaped; the editor shows the
  effective request and folds edits back into the active variation (all five
  fields) or, with none active, into the base.

## Consequences

### Positive

- POS-001: One file per request: atomic writes, one git change, duplicate/
  move/delete need no second file.
- POS-002: Files without variations are byte-identical; old files load.
- POS-003: Whole-field replacement has no merge rules to explain or get wrong.

### Negative

- NEG-001: Switching the active variation is a file change (user's choice:
  the team shares the selection).
- NEG-002: Voleeo or an older Yuki that rewrites the file drops `variations`.
- NEG-003: Every walker of `request.auth` (seal/open/mask/export/import) must
  also walk variation auth — served by one `auths_mut()` helper.
- NEG-004: A variation duplicates all five fields in the YAML; a change meant
  for every variation (a new header) has to be made in each.

## Alternatives Considered

### Variables-only variations

- ALT-001: Description: a variation is a named `{{ VAR }}` set that beats env
  and folder vars; the request templates the varying parts.
- ALT-002: Rejection reason: user chose whole-field overrides.

### Session-local active selection

- ALT-003: Description: active variation in `session.yaml`; switching causes no
  git diff.
- ALT-004: Rejection reason: user wants the selection shared in the request
  file.

### Sidecar `req_{id}.variations.yaml`

- ALT-005: Rejection reason: a second file per request on every lifecycle op
  (NEG-001 of the notes ADR) with no readability gain.

## References

- REF-001: docs/features/FEAT-20261015-request-variations.md
- REF-002: docs/adr/ADR-20261009-request-notes-sidecar.md
