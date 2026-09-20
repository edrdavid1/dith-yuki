# Crash recovery & atomic save

As-built notes for the Journal / Crash Recovery track. Complements the Unsaved Changes Guard (quit / close prompts): the guard cannot help after `kill -9`, panic, or power loss — the disk journal can.

## What ships

| Piece | Role |
|-------|------|
| **Atomic write** (`engine-io::atomic_write`) | temp → fsync → rename / `MoveFileEx`; used by project Save and journals |
| **Recovery journal** | Debounced full `.dyproj` snapshot under `{app_data}/recovery/` while a doc is dirty |
| **Clean-exit marker** | Written only after QuitGuard allows exit; absent ⇒ previous run was unclean |
| **Soft discard** | Tab “Don’t Save” flushes journal + Restore toast (~12s) |
| **Session roster** | `{app_data}/session_roster.json` — last open tabs; reopen prompt when unclean and journals empty |
| **Terminate signals** | Best-effort journal flush on SIGTERM / console ctrl (not SIGKILL) |

## Layout on disk

```
{app_data_dir}/
  clean_exit.marker          # present only after a guarded clean quit
  session_roster.json        # open tabs snapshot (cleared on clean exit)
  recovery/
    {uuid}.dyproj.journal    # full project zip (same format as Save)
    {uuid}.meta.json         # display name, paths, content_kind, discarded, …
```

`recovery_id` is a UUID on each `DocumentSession` (stable across process restarts). Runtime `doc_id` (`u32`) is not used in filenames.

## When the journal writes

1. **Debounce ~3s** after dirty / mutation (not every preview frame).
2. **Heartbeat ~30s** if still dirty and the journal is older than 30s.
3. **Soft discard** — synchronous flush before closing a dirty tab with Don’t Save.
4. **Terminate signals** — best-effort flush of dirty docs.

After a successful **Save / Save As**, that session’s journal + meta are deleted.

**Cap:** documents larger than `JOURNAL_MAX_BYTES` (512 MiB) skip the full journal (`content_kind: skipped_too_large`); atomic Save and the clean-exit marker still apply.

## Startup UI

`scan_recovery_journals` returns `{ previous_unclean, journals, roster }`.

| Condition | UI |
|-----------|-----|
| `journals.length > 0` | **Recover unsaved work?** — Recover all / Discard journals / Open originals only |
| unclean + empty journals + roster entries | **Reopen previous session?** — Reopen paths / Skip |
| otherwise (incl. first launch with no journals) | Normal shell |

First launch also lacks `clean_exit.marker`; recovery is shown only when journals or an unclean roster exist.

## Soft discard vs Unsaved Guard

- **Guard** — Save / Don’t Save / Cancel on quit and tab close.
- **Soft discard** — Don’t Save on a dirty tab still writes a journal and offers **Restore** briefly so accidental discard is reversible within the session window.
- Journals survive force-quit; the Restore toast does not.

## IPC (frontend)

| Command | Purpose |
|---------|---------|
| `scan_recovery_journals` | Startup gate payload |
| `recover_journal` | Open journal bytes as a new tab |
| `discard_recovery_journals` | Delete selected or all journals |
| `prepare_soft_discard` | Flush + mark discarded before close |

Events: existing `dirty-changed` / `tabs-changed` drive journal scheduling and roster persistence.

## Limits & non-goals

- Not a substitute for user Save; journals are crash insurance, not autosave-as-Save.
- SIGKILL / hard power cut can still lose the last debounce window (up to ~3s, or ~30s if only heartbeat was due).
- Tiered / light sidecar journals for huge docs (spec mode B) are not implemented; only full + skip-by-cap (A+C).

## Related code

- `crates/engine-io/src/atomic_write.rs`
- `src-tauri/src/journal/` (`runtime`, `write`, `clean_exit`, `roster`, `signals`, `commands`)
- `frontend/src/components/RecoveryDialog.tsx`, `DiscardRestoreToast.tsx`
- `frontend/src/App.tsx` (startup gate)
- Spec (local only): `.local-doc/JOURNAL_CRASH_RECOVERY_spec.md`
