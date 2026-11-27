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

## Background sync benchmark (measured 2026-11-27)

Reproduce: `FALCON_BENCH=1000,5000,10000 FALCON_RUNS=2 FALCON_LEGACY=1 cargo test -p falcon-library --release --lib background_sync::tests::bench -- --ignored --nocapture`
(an idle machine; nothing else compiling). The fixture is small PNGs spread over 20 subfolders in a
throwaway library kept in the app's batched write mode; a probe thread takes the library lock like an
interface frame (lock, flush what is pending, unlock) every 2 ms and records how long it waited.
CPU and RSS come from `ps`. 1,000 and 5,000 files: 2 runs; 10,000 files: 1 run (each takes minutes).
Defaults: 3 staging threads, commits in groups sized to a 40 ms lock hold, rest 15% of a hold (at least 2 ms).

| mode | files | runs | duration s (median) | files/s | UI lock wait p50 / p95 / p99 ms (median of runs) | max wait ms (per run) | CPU % | RSS MB | cancel ms (per run) |
|---|---|---|---|---|---|---|---|---|---|
| Copy | 1000 | 2 | 18.6 | 54.0 | 0.0 / 36.1 / 42.7 | 53, 59 | 26 | 0 | 14, 11 |
| Copy | 5000 | 2 | 88.8 | 56.6 | 0.0 / 35.2 / 40.8 | 129, 52 | 23 | 2 | 13, 21 |
| Copy | 10000 | 1 | 182.9 | 54.7 | 0.0 / 35.5 / 41.3 | 148 | 23 | 2 | 19 |
| Link | 1000 | 2 | 53.4 | 18.8 | 0.0 / 45.7 / 51.8 | 62, 126 | 22 | 13 | 17, 19 |
| Link | 5000 | 2 | 237.8 | 21.1 | 0.0 / 42.8 / 49.1 | 708, 67 | 22 | 38 | 14, 14 |
| Link | 10000 | 1 | 480.0 | 20.8 | 0.0 / 42.8 / 49.1 | 390 | 24 | 53 | 15 |

Unchanged (no-op) pass, seconds for passes 1,2,3 of the last run, and CPU%:
- Copy 1000: 0.014, 0.007, 0.006 s, CPU 68%
- Copy 5000: 0.029, 0.030, 0.027 s, CPU 91%
- Copy 10000: 0.074, 0.050, 0.051 s, CPU 91%
- Link 1000: 0.174, 0.140, 0.143 s, CPU 100%
- Link 5000: 2.760, 0.989, 0.927 s, CPU 63%
- Link 10000: 2.467, 1.542, 1.481 s, CPU 88%

Previous behaviour (whole pass under one lock hold), 1,000 files:
- Copy 1000: 10.3 s (96.7 files/s); the interface waited 10.3 s for the lock
- Link 1000: 24.0 s (41.6 files/s); the interface waited 24.0 s for the lock


The occasional large "max wait" (708 ms once at 5,000 Link, 390 ms at 10,000 Link, 205 ms at one Copy run)
did not repeat in the repeat runs and is not tied to one phase; they look like storage stalls, and the
cause was not isolated. Medians and p99 are the figures to read.

Lock-hold budget at 1,000 files (one run each): what the interface waits versus how long the pass takes.

| budget | Link: time / p95 / p99 wait | Copy: time / p95 / p99 wait |
|---|---|---|
| 15 ms | 47.4 s / 41.8 / 47.7 ms | 26.6 s / 24.1 / 29.0 ms |
| 40 ms (default) | 53.4 s / 45.7 / 51.8 ms | 18.6 s / 36.1 / 42.7 ms |
| 100 ms | 32.4 s / 83.3 / 94.6 ms | 15.3 s / 93.3 / 105.3 ms |
| 250 ms | 25.5 s / 161.8 / 184.1 ms | 12.2 s / 83.7 / 97.3 ms |
| whole pass under one lock hold (previous) | 24.0 s / the interface was blocked for all of it | 10.3 s / blocked for all of it |

Reading it: the committer's cost is the library's durable commit (about four fsyncs per file), so the
background pass is roughly 1.7 to 2.3 times slower in wall-clock than the previous one-lock pass, in
exchange for an interface that never waits more than about a hold. A larger budget gets closer to the
old speed and costs responsiveness. The unchanged pass at 10,000 files costs about 0.05 s (Copy) and
about 1.5 to 2.5 s of one core (Link, which reads every sidecar), every five minutes per automatic
folder, and writes nothing.
