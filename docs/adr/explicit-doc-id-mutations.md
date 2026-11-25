# ADR: Explicit `doc_id` on mutating IPC

**Status:** Accepted  
**Date:** 2026-11-23  
**Related:** [multi-doc-tabs.md](../multi-doc-tabs.md) (T4)

## Context

With N open documents in one process, resolving mutations through
`active_session()` races the tab strip: a filter/save/undo issued against
document A can land on B if the user switches tabs before the command runs.

## Decision

1. Every command that changes document state takes an explicit runtime
   `doc_id: u32`.
2. The backend resolves with `AppState::require_session(doc_id)` — never
   substitutes the active tab.
3. Missing session → error (`Document was closed; …`); no silent no-op that
   looks like success.
4. Chrome / viewport reads may still use `active_session()`; calls in
   `commands/` without `// active-session-ok:` fail
   `npm run lint:no-active-session`.

## Consequences

**Positive**

- Cross-tab races cannot rewrite the wrong project.
- Frontend can capture `docId` at the user action and pass it through IPC.
- Lint prevents regression of the magic-active path in command handlers.

**Negative**

- Every mutating IPC signature and frontend wrapper must carry `docId`.
- Read helpers that stay on active can still show stale chrome for one frame
  after a switch (acceptable; mutations are the data-loss class).

## Alternatives rejected

- Queue mutations until the issuing tab is active again — stalls UX and
  complicates undo.
- Soft-fail / ignore when `doc_id ≠ active` — hides bugs; save/export must
  still work on background tabs after an explicit activate.
