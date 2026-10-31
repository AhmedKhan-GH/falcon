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

`folders` is a recursive tree. Eagle rebuilds every node in a fixed key order each time it saves, and leaves out keys whose value is unset. The fixture's folders carry only the always-present keys.

| Key | Type | Notes |
|---|---|---|
| `id` | string | See [IDs](ids.md) |
| `name`, `description` | string | `description` is `""` when empty |
| `children` | array | Child folders, in sidebar order |
| `modificationTime` | integer | ms. Set when the folder is created or duplicated. Renames, icons, sorting, passwords and moves leave it alone (observed) |
| `tags` | string array | Tags given to items added to the folder |
| `extendTags` | string array | Runtime only. The folder's tags plus its ancestors', worked out on load. The renderer's save includes it, but the writer deletes it before writing, so it shouldn't appear on disk |
| `icon` | string | Optional. A name from the icon picker (see below). No key means the default folder icon |
| `iconColor` | string | Optional. `red`, `orange`, `yellow`, `green`, `aqua`, `blue`, `purple` or `pink`. No key means the default grey |
| `pinyin` | string | Runtime only. A romanised name cached for search, which the writer also deletes |
| `password` | string | Always present. `""` when unset (see passwords below) |
| `passwordTips` | string | Always present. The hint, `""` when unset |
| `coverId` | string | Optional. The item shown as the folder's cover |
| `orderBy` | string | Optional. The folder's own sort (see below) |
| `sortIncrease` | bool | Written with `orderBy`. `true` is ascending |

**Sort keys (observed):** `IMPORT`, `MTIME`, `BTIME`, `NAME`, `EXT`, `FILESIZE`, `RESOLUTION`, `RATING`, `DURATION` and `RANDOM` are in the folder menu. `TAGS` is in the main sort menu, and `MANUAL` (a hand-arranged order, kept per folder in each item's `order` map) is honoured when present. No `orderBy` key means the folder follows the global sort. Choosing a sort sets `sortIncrease` to `true` if it was unset. Clearing it deletes both keys.

**Icons (observed):** the picker offers 161 names. Falcon lists them in `falcon_library::model::FOLDER_ICONS`, spelled exactly as Eagle stores them (including `excalmation`, `airplan` and `rophy`). Picking the first, blank entry deletes the key. A new subfolder takes its parent's `icon` and `iconColor`.

**Passwords (observed):** `password` is `btoa(password)`, plain base64 with no hashing. `btoa` reads each UTF-16 unit as one byte and throws on anything above U+00FF. Eagle doesn't escape the text first, so a password with such characters can't be set at all. `é` is stored as `6Q==`, not as the base64 of its UTF-8 bytes. Unlocking checks `atob(password)` against the typed text. Removing a password deletes both keys from the in-memory folder, but the save writes `password || ""`, so on disk a folder that never had a password and one whose password was removed look the same: `"password":"","passwordTips":""`. Falcon refuses empty and non-Latin-1 passwords.

**Ordering:** the array order is the sidebar order. Dragging a folder between siblings moves it in its parent's `children`. Dropping a folder onto another one appends it to that folder's children.

**The writer (observed):** the background process writing `metadata.json` deletes these runtime keys from every folder: `covers`, `images`, `folders`, `newFolderName`, `editable`, `pinyin`, `__root`, `extendTags` and `$$hashKey`. From smart folders it deletes only `newFolderName`, `editable` and `$$hashKey`. It then writes `{folders, smartFolders, quickAccess, tagsGroups, modificationTime: Date.now(), applicationVersion}`. It refuses to write, and reports a sync conflict, when the file's `modificationTime` is newer than the one Eagle loaded. Falcon still models `extendTags` and `pinyin` so that a file holding them round-trips.

**Names (observed):** every save NFC-normalises folder, smart-folder and tag-group names. Renames are cut to 1024 UTF-16 units; falcon cuts at 1024 characters. Falcon normalises and cuts every name it writes. A rename drops any stale `pinyin`.

### Tag groups (observed in code)

`tagsGroups` is an array in sidebar order. The fixture has none. Each save rebuilds every entry as:

| Key | Type | Notes |
|---|---|---|
| `id` | string | Same generator as item IDs (time in base 36 plus 5 random characters) |
| `name` | string | NFC-normalised. Renames are cut to 1024 characters |
| `tags` | string array | Members in group order. Line breaks are stripped when tags are filed |
| `color` | string | Optional, written only when set. Same palette as folder `iconColor` |
| `description` | string | Optional, written whenever it isn't `undefined`, so `""` survives |

- **Creating a group:** a new group goes at the end. Creating one from selected tags leaves those tags in their other groups.
- **Adding tags:** adding through the tag manager moves each tag, taking it out of any other group, so a tag normally belongs to one group.
- **Removing tags:** taking tags out of a group leaves them on their items.
- **Deleting a group:** Eagle asks whether to delete the group's tags from every item too.
- **Reordering:** the sidebar is drag-sortable, and the array follows it.
- **Other tag edits:** renaming or deleting a tag also updates every group.

None of these edits touch the folders' `modificationTime`. The library's top-level `modificationTime` is stamped when `metadata.json` is written.

## `saved-filters.json` (observed in code)

A JSON array in menu order, `[]` in the fixture. Eagle writes it compact with `JSON.stringify` and no trailing newline. Each entry is `{"name": …, "rule": {…}}`, with no id and no timestamp, and Eagle finds entries by position. Saving a filter first re-reads the file, then appends. A rename ignores empty names. The menu is drag-sortable, and the array follows it.

`rule` is a deep copy of the filter panel's rules, made by `JSON.parse(JSON.stringify(defaults))` and then edited, so `undefined` values are dropped and integer-like keys come first (`rating` is written `"0"` to `"5"`, and `shape` starts with `"34","43","169","916"`). `keyword` is added at the end when a search was active. Applying a filter replaces each top-level rule key, a shallow `{...defaults, ...rule}`. The tag and folder logic (OR, AND or EQUAL) is a panel setting that isn't saved.

| Rule key | Shape | Falcon `search::Filter` |
|---|---|---|
| `type.includes`, `type.excludes` | `{ext or group: true}` | `types`, `exclude_types` |
| `tag.includes`, `tag.excludes` | string arrays | `tags.includes`, `tags.excludes` |
| `tag.no` | bool | `tags.empty` |
| `folder.includes`, `folder.excludes` | `{id: folder object}`. Only `.id` is read back. The key `NoFolders` means items in no folder | `folders.includes`, `folders.excludes`, and `NoFolders` in includes maps to `folders.empty`. `NoFolders` in excludes has no counterpart |
| `rating` | `{"0"…"5": bool}`, `0` = unrated | `ratings` |
| `shape.portrait`, `landscape`, `square`, `panoramicPortrait`, `panoramicLandscape` | bool | `shapes` |
| `shape.34`, `43`, `169`, `916`, `custom` (+ `width`, `height`) | bool, numbers | none yet |
| `color.value` | `[r, g, b]`, absent when off | `color.rgb` |
| `color.accuracy` | number, default 20 | `color.accuracy` |
| `color.gray` | bool | `grayscale` |
| `resolution.minW`, `maxW`, `minH`, `maxH` | number or numeric string | `width`, `height` |
| `file.min`, `file.max`, `file.unit` | number in `kb` or `mb` (`× 1024` or `× 1024²`, then `parseInt`) | `size` in bytes |
| `url`, `note` | `{has, no, keywords}`. `keywords` is comma-separated, and all must match | `url`, `note` (`TextFilter`) |
| `annotation` | `{has, no, keywords}` over the item's `comments` | none yet |
| `import`, `mtime` | `today`, `yesterday`, `last7day` … `last365day`, `usingRange`, `range`, `type`, `model`, `selectedMonths` | none yet |
| `duration` (`unit` `s`/`m`/`h`), `bpm`, `font`, `camera`, `image`, `semantic` | various | none yet |
| `keyword` | string | returned beside the filter |

Falcon's `SavedFilter::to_filter` returns the filter, the keyword and a list of active rules it couldn't express. `SavedFilter::rule_from_filter` builds the full default rule object with falcon's rules filled in. It writes sizes in KB, and folder entries as `{id, name}`, since Eagle only reads `id` back. Both directions are falcon's own reading of Eagle's code, checked by unit tests against hand-built rules, not against a file Eagle wrote (inferred).

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

## Export (`.eaglepack`)

An `.eaglepack` is how Eagle moves items and folders between libraries.

```text
Refs.eaglepack           a zip archive, entries stored without compression
├── pack.json            {"images": [...], "folder": {...}}
└── <ID>.info/           each item's whole info folder, as in images/
    ├── metadata.json
    ├── <name>.<ext>
    └── <name>_thumbnail.png
```

| Behaviour | Status | Detail |
|---|---|---|
| Container | observed | Zip, written by Eagle's archiver with `store: true` (no compression). The 7-Zip binaries Eagle ships are not used for packs |
| Entry order | observed | `pack.json` first, then each item's `<ID>.info` folder in `images` order |
| `pack.json` keys | observed | `images` first; `folder` only for a folder export |
| `images` | observed | Item records (the item `metadata.json` contents, minus Eagle's in-memory fields), sorted by `modificationTime`, newest first |
| Exporting items | observed | Every record gets `"folders": []`. Trashed items may be exported; their record loses `isDeleted` |
| Exporting a folder | observed | Untrashed items in the folder or any subfolder; their `folders` keep the original ids, including folders outside the export. `folder` is a copy of the subtree with `id`, `name`, `description`, `children`, `modificationTime`, `tags`, then `extendTags`, `icon`, `iconColor`, `pinyin` when set, `password`, `passwordTips`, `coverId` when set, and `orderBy` with `sortIncrease` when a custom sort is set |
| Duplicate records | inferred | Eagle's export means to skip items listed twice, but the map it checks is never filled, so an item in a folder and its subfolder may be listed twice. falcon lists each item once |
| Names | observed | Eagle skips items whose name contains characters the file system rejects. falcon's names are already sanitized at import |
| Verification | observed | After writing, Eagle checks the zip signature (`PK\x03\x04` and its variants) and that the file is at least 80% of the bytes written; otherwise it deletes the file. falcon writes to a temporary file, reads it back (signature, entry count, `pack.json` parses) and only then moves it into place |
| Import: extract | observed | Unzipped (with `unzip -o` on macOS, which keeps file times) into a temporary folder; fails without `pack.json` |
| Import: order | observed | `images` is reversed (oldest first). For a folder pack, items are sorted by `order[<old folder id>]`, falling back to `modificationTime` |
| Import: ids | observed | Every item and folder gets a new id; nothing keeps its old one. Folders are marked `isExpand: true` and `coverId` follows the item's new id |
| Import: folders | observed | A folder pack's tree is added to the library and item `folders` are mapped to the new ids. A pack of items dropped on a folder goes into that folder; otherwise items keep their `folders` |
| Import: records | observed | Each item's `metadata.json` is rewritten from its `pack.json` record with the new `id` and `modificationTime` = now + index; `lastModified` is kept |
| Import: duplicates | observed | Eagle asks whether to skip items that match existing ones by its duplicate key: for most images `size + width + height + ext`, for videos `size + duration`; SVG, TIFF and some other kinds never match |
| Smart folders | observed | Exporting a smart folder writes a folder pack of its matching items, but those items' `folders` don't point into the exported tree, so they import with no folder. Smart folder rules are never carried |

### How falcon does it

`falcon_library::pack::export_pack(&lib, PackSource::Items(ids) | PackSource::Folder(id), dest)` writes a pack following the table, using the `zip` crate with stored entries and each file's time and Unix mode. `import_pack(&mut lib, pack, &PackImportOptions { folder, skip_duplicates })` imports one and returns the new item ids. Item files are moved in as they are (thumbnail included), not re-imported, as Eagle does. Differences from Eagle, all inferred to be harmless:

- falcon drops item `folders` ids that don't exist after import, where Eagle would write `null` or keep a dangling id.
- An item whose `<ID>.info` folder is missing from the pack is skipped; Eagle still lists it, with no files.
- A record without `isDeleted` (an exported trashed item) is written back with `"isDeleted": false`, since falcon's item model requires the key.
- The duplicate key covers images and videos only; audio and font keys are not implemented.
- Smart folder export isn't offered.

The CLI has `falcon export <library> <out.eaglepack> [--folder ID | ITEM...]` and `falcon import-pack <library> <pack> [--folder ID] [--skip-duplicates]`.
