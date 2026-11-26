# Owner build: differences from upstream Falcon

This repository is the Owner's local development line. It is for personal and
internal production use. **Nothing here is pushed or submitted upstream unless
the Owner decides so**: the repository has no remote configured, and the
Antigravity worktrees it was recovered from are kept as they were.

## Baseline and history

- Upstream base: `main` / `40636d9` (released as v0.1.4).
- Owner integration line: `integrate/falcon-dev-unified` (`a53ca31`), 34 commits
  and ~18.5k lines ahead of upstream `main`.
- Recovered 2026-11-27 from the Antigravity worktree `falcon-git` (all branches,
  tags and history kept; its remote-tracking refs are kept under
  `refs/archive-remotes/`). Recovery work happens on `recovery/*` branches.
- The older `FALCON/` backup folder was an incomplete copy (no `.git`, nine
  files of `a53ca31` missing, 105 others out of date). It is not a source of truth.

## Custom features (all absent upstream)

| Feature | Where it started |
|---|---|
| Referenced assets (Link mode), sidecars, journal, fault-injection seam | `1af993e`, `4c11e6b`, `ed9ba46` |
| Library Settings, status badges, safe edit/delete guards | `37d0db6` |
| Missing media and Relink (single and batch) | `074d7f4` |
| QA isolation (`FALCON_QA_DIR`) and Falcon Dev app-data isolation | `b5479d7`, `8741c0b` |
| Adaptive preview pipeline, zoom slider fixes | `de2a018`, `62f6ae9`, `085214b`, `ce3c257` |
| Move Library (cross-folder, cross-volume) | `0f62692`, `e82dd53` |
| Recursive subfolder counts | `dbbe0e0` |
| Affinity embedded previews | `43e305b` |
| Finder drag-out preview fix | `af71415` |
| Deferred background thumbnail worker for Link imports | `46dcf91` |
| Loader memory budget (128 MB LRU) | `ef0b570` |
| Hide Unsupported Files, format capability registry | `925c321` |
| Bahasa Indonesia and Basa Bali locales | `8cda35e`, `58d1d49` |
| Path bar, viewer sidebar exit, folder sync (watched folders) | `a53ca31` |

## Recovery-branch changes (`recovery/2026-11-27`)

- **Move Library is now a recoverable transaction** (`falcon-app/src/library_move.rs`):
  uniquely owned `.falcon-move-<token>` staging folder, blake3 content manifest
  verified from the destination, refusal if the library changes while copying,
  openability check, fsync, atomic same-folder rename that cannot replace a
  non-empty folder, cleanup limited to the folder this call created, and
  fault-injection tests at every stage.
- **Folder sync** (`falcon-library/src/folder_sync.rs`): status always ends in a
  terminal state (also after a crash: a stored `Syncing` reads as an interrupted
  error); precise temporary-file rules (`design.tmpl.png` syncs, `x.png.tmp`
  does not); Copy mode keeps a stable source record so renames never re-import
  and a changed source is a reported conflict, never an overwrite; Link mode
  refreshes everything derived from a changed source and keeps tags, notes,
  folders and id. Deferred thumbnails staged from an old source are refused.
- **QA sandbox is enforced at every open/create** (`install_dir::guard`), not
  only at startup.

## Compatibility notes for future updates or contributions

- `falcon.json` gained `syncSources` (Copy-mode provenance). It is optional and
  skipped when empty; an older Falcon ignores it and a copy without it falls
  back to adopting matching items.
- `FalconConfig`, `SyncStats` (new `conflicts`, `added_ids`, `refreshed_ids`),
  `FaultPoint` (new `Sync*` points) and `StagedThumbnail` (new `source_identity`)
  grew; callers that build them literally need the new fields.
- The tree is not `rustfmt`-clean upstream and CI does not run it; do not
  reformat whole files, it would bury real differences.
