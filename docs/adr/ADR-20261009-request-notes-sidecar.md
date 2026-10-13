---
title: "ADR-20261009-request-notes-sidecar: Request notes live in a sidecar req_{id}.md"
status: "Accepted"
date: "2026-10-09"
authors: "user + agent"
feature_id: FEAT-20261009-request-notes
scope: "persistence"
tags: ["storage", "on-disk-format", "git", "request"]
supersedes: ""
superseded_by: ""
---

# ADR-20261009-request-notes-sidecar: Request notes live in a sidecar `req_{id}.md`

## Status

Accepted

## Context

Requests gain free-form markdown notes. Workspace files belong to the user and
are committed to git (one repo per workspace). The notes must be readable and
diffable outside Yuki.

## Decision

Store a request's notes verbatim in `workspaces/{ws}/req_{id}.md`, next to
`req_{id}.yaml`. No field is added to `HttpRequest` or its YAML. `RequestStore`
owns the file's lifecycle: `get_notes`/`save_notes` (empty notes delete the
file), `copy_request` copies it, `delete_request` removes it.

## Consequences

### Positive

- POS-001: GitHub and any editor render the notes as markdown.
- POS-002: Request YAML schema and Voleeo compatibility are unchanged.
- POS-003: Notes load lazily (only for open tabs), the tree stays cheap.

### Negative

- NEG-001: Every request file operation must handle a second file; the store is the single chokepoint (`copy_request`, `delete_request`).
- NEG-002: Tools reading only the YAML (exporters, `request.list`) don't see notes; MCP `request.get`/`create`/`update` go through `get_notes`/`save_notes`.
- NEG-003: Editing a request and its notes produces two file changes per commit.

## Alternatives Considered

### `description` field inside `req_{id}.yaml`

- ALT-001: Description: optional multi-line field on `HttpRequest`, same name as Yaak/Postman/Insomnia.
- ALT-002: Rejection reason: user preferred plain `.md` files editable and rendered outside Yuki.

## Implementation Notes

- IMP-001: `yuki-git::classify_path` maps `req_{id}.md` to the request node.
- IMP-002: `list_requests` already filters on `.yaml`, so the sidecar is invisible to it.

## References

- REF-001: docs/features/FEAT-20261009-request-notes.md
