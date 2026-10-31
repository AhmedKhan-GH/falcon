# Library format

An Eagle library is a folder ending in `.library`. All state is plain JSON next to the files themselves. There is no database.

```text
Assets.library/
├── metadata.json        folder tree and library-wide settings
├── mtime.json           item id → modification time, plus "all"
├── tags.json            historyTags, starredTags
├── saved-filters.json
├── backup/              JSON snapshots
├── Icon                 empty file (the macOS custom-icon marker)
└── images/
    └── <ID>.info/
        ├── metadata.json            one item
        ├── <name>.<ext>             the original file
        └── <name>_thumbnail.png     only for some items
```

## Library `metadata.json` (observed)

The top-level keys, in this order, are `folders`, `smartFolders`, `quickAccess`, `tagsGroups`, `modificationTime` and `applicationVersion`.

`folders` is a recursive tree. Each node has `id`, `name`, `description`, `children`, `modificationTime`, `tags`, `password` and `passwordTips`.

## Item `metadata.json` (observed)

| Key | Type | Notes |
|---|---|---|
| `id` | string | See [IDs](ids.md) |
| `name` | string | File name without the extension |
| `size` | integer | Bytes |
| `btime`, `mtime` | integer | Times in ms, taken from the original file |
| `ext` | string | No leading dot |
| `tags`, `folders` | string arrays | `folders` holds folder IDs |
| `isDeleted` | bool | In the trash |
| `url`, `annotation` | string | |
| `modificationTime` | integer | Time the record changed, in ms |
| `removeThumbnail`, `noThumbnail` | bool | Optional: present on 72 of the fixture's 91 items |
| `height`, `width` | integer | Pixels. Three items write `width` first |
| `lastModified` | integer | ms |
| `palettes` | array | `{color: [r,g,b], ratio}`. See [palettes](palettes.md) |

## `mtime.json` (observed)

An object that maps item IDs to ms times, ending with `"all"`. In the fixture it lists only 32 of the 91 items, yet `all` is 91. So `all` is the library's total item count, not the number of entries.

## Encoding (observed)

Eagle writes compact JSON with no trailing newline and keeps keys in insertion order.

## How falcon does it

`falcon_library::Library::open` loads every JSON file into typed structs. `Library::save_to` writes them back out.

- **Lossless:** unknown keys are kept in an `extra` map on each struct.
- **Number format:** `ratio` stays a raw `serde_json::Number`, so `45` isn't rewritten as `45.0`.
- **Key order:** each item remembers whether `width` came before `height`.

Together these make every JSON file in the test fixture re-serialize byte-for-byte, which the `round_trips_byte_identically` test checks.

## Backups

Eagle keeps snapshots of the library `metadata.json` in `backup/`. Only that one file is backed up; item `metadata.json` files, `mtime.json` and `tags.json` are not.

| Behaviour | Status | Detail |
|---|---|---|
| File name | observed | `backup-YYYY-MM-DD HH.mm.ss.SSS.json` in local time, with dots between the time fields, e.g. `backup-2026-10-26 02.15.25.310.json` (fixture and Eagle's code) |
| Contents | observed | A byte copy of `metadata.json` |
| When | observed | After Eagle rewrites `mtime.json` because something changed, and on a 30-minute timer. Not on every `metadata.json` write |
| Damaged source | observed | No backup when `metadata.json` doesn't parse or has no truthy `applicationVersion` |
| Unchanged check | observed | Skipped when the newest backup has the same **size** as `metadata.json` (the content isn't compared) |
| Which files count | observed | Any name containing `backup-`; sorted by name, newest first |
| Rotation | observed | At most 100. Eagle trims the list it read before copying to its newest 99, so with the new copy 100 remain. Trimming also runs when the copy was skipped |
| Second copy | observed | Eagle also copies `metadata.json` to `eagle-backup/<hash of library path>.json` in its app-data folder. falcon doesn't (it is outside the library) |
| Recovery | observed | When `metadata.json` exists but can't be read or parsed (or has no `applicationVersion`), Eagle walks the backups newest first, skips empty ones and ones that don't parse or lack a truthy `modificationTime`, keeps the damaged file as `backup/metadata.json.damaged-<ms>`, and copies the first good backup over `metadata.json`. A missing `metadata.json` is reported as a missing library instead |

### How falcon does it

`falcon_library::backup` follows the table. `Library::backup_metadata()` takes a backup now (returning `None` when skipped), `Library::backups()` lists them newest first, and `Library::restore_metadata_from_backup(path)` restores a chosen one. `Library::open_or_restore(path)` opens a library and, when `metadata.json` can't be read or parsed, restores the newest usable backup first, as Eagle does on launch. falcon also requires the backup to load as a `LibraryMetadata`, slightly stricter than Eagle's `modificationTime` check (inferred to be harmless: a backup without the standard keys would break Eagle too).

`Library::save_metadata` does not take backups, because Eagle doesn't back up on each write. Callers (the app on change or a timer, or `falcon backup <library>`) call `backup_metadata()`.
