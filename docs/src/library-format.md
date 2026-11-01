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
├── icon.png             the library's own icon (optional)
└── images/
    └── <ID>.info/
        ├── metadata.json            one item
        ├── <name>.<ext>             the original file
        └── <name>_thumbnail.png     only for some items
```

`icon.png` is the picture the library switcher shows beside the library's name. Falcon's Change Icon scales the chosen image so its shorter side is 64 px, saves it as PNG next to the library's other files and renames it over `icon.png`, so an old icon is never left half written. Remove Icon deletes the file. Both go through the sandbox check. The HTTP API serves the same file.

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

**Sort by title (observed in code):** a folder's context menu has a "Sort By" submenu with four commands: title A→Z or Z→A, for the **current level** or for **all levels**. The current level is the clicked folder's siblings, that is, its parent's `children` (or the root list). All levels sorts the root list and then every `children` array in the tree. Eagle asks for confirmation, sorts the arrays in place and saves. No flag is stored, and `modificationTime` is left alone. The comparison lowercases both names and uses `localeCompare` with numeric ordering, so `folder 2` comes before `Folder 10`. Z→A is the A→Z result reversed.

Falcon: `Library::sort_subfolders(parent, SubfolderSort { descending, all_levels })` sorts `parent`'s subfolders (`None` for the root list), and with `all_levels` every level below them too. Pass the clicked folder's parent for Eagle's "current level", and `None` with `all_levels` for its "all levels". Names compare with `search::natural_cmp`, the comparison falcon also uses for title sorts. Unlike the ICU collation Eagle uses, it orders accented letters by code point (inferred to matter only for non-ASCII names). Eagle's comparator calls an empty name equal to everything. Falcon sorts empty names first, because Rust's sort needs a consistent order.

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
- **Reordering:** the sidebar is drag-sortable, and the array follows it. The drag takes the group out of the array and inserts it at the drop index, which is counted after the removal. This is what falcon's `reorder_tag_group(id, index)` does.
- **Description:** the tag manager shows an editable box under a selected group's name. On blur, Eagle saves the box's HTML if it changed: a trailing `<br>` and any `<div>` wrappers are stripped, and `&amp;` is turned back into `&`. Clearing the box writes `""`, so the key stays once it has been set. Groups made by the tag manager have no `description` key until then. Falcon's `set_tag_group_description(id, text)` writes the given string, `""` included.
- **Other tag edits:** renaming or deleting a tag also updates every group.

None of these edits touch the folders' `modificationTime`. The library's top-level `modificationTime` is stamped when `metadata.json` is written.

## `tags.json` (observed in code)

`{"historyTags":[...],"starredTags":[...]}`, compact, with no trailing newline. Eagle writes it five seconds after a change, through an atomic write.

- `historyTags`: recently used tags, latest first, at most 120. Adding a tag (the tag picker, pasting tags, the inspector) moves it to the front; adding several moves each in turn, so the last one leads. Eagle's imports with tags move them as a batch, so there the first one leads (falcon's imports leave the history alone). A tag taken off its last item is dropped. Renaming or merging a tag renames it in place, or drops it when the new name is already listed (the same rule holds for `starredTags` and each tag group's `tags`); deleting a tag removes it. Clear Tag History empties the list.
- `starredTags`: starred tags, most recently starred first.

Tag groups are not in this file but in `tagsGroups` of the library `metadata.json` (see above).

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
| `import`, `mtime` | `today`, `yesterday`, `last7day` … `last365day` and `usingRange` (bools), `range` (`[start, end]` local midnights in ms), `type` (`"range"` once a range is picked), `model` (the picker's text), `selectedMonths` (`{"YYYY/MM": true}`). The panel has one range picker, so the range lives in `import.range` and Date Modified's `usingRange` reads it from there | `imported`, `modified` (`DateFilter`: presets, `range`, `months`) |
| `btime` (falcon only) | same shape as `import`, written only when Date Created is on; Eagle ignores the key | `created` |
| `duration` | `min`, `max` (number or numeric string) in `unit` `s`, `m` or `h` | `duration` in seconds |
| `bpm` | `min`, `max` | `bpm` |
| `camera` | `{camera name: true}` | `cameras` |
| `font` | `{activated, deactivated}` bools; `activated` wins | `font` (`FontFilter`) |
| `image`, `semantic` | image search and natural-language search | none yet |
| `keyword` | string | returned beside the filter |

Falcon's `SavedFilter::to_filter` returns the filter, the keyword and a list of active rules it couldn't express. `SavedFilter::rule_from_filter` builds the full default rule object with falcon's rules filled in. It writes sizes in KB, and folder entries as `{id, name}`, since Eagle only reads `id` back. Durations go in the largest of `h`, `m` and `s` that keeps both bounds whole. A Date Imported or Date Modified range goes in `import.range` with `import.type: "range"`, where Eagle reads both; only when the two rules have different ranges does falcon also write `mtime.range`, which it reads back first and Eagle ignores (Eagle then applies the imported range to both). A range with an unset end reads as `(0, 0)`, which passes nothing, as in Eagle. Reading a font rule fills the `FontFilter` with the fonts in `~/Library/Fonts/EagleApp` (`to_filter_with_fonts` takes them from the caller). Both directions are falcon's own reading of Eagle's code, checked by unit tests against hand-built rules, not against a file Eagle wrote (inferred).

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

**Files Eagle can't preview.** For items such as `.tex`, `.md`, `.py`, `.csv` and `.epub` files, Eagle leaves out `height`, `width` and `palettes`. It may also add keys such as `noPreview` and `deletedTime` before `lastModified`. Seen in real libraries: 1,456 of 2,632 items in one, 2 EPUBs in another.

**Damaged records.** A record can be unreadable, for example 604 zero bytes after a cloud sync went wrong (seen once in a real library).

Falcon treats every key but `id` as optional. It remembers each record's key order and writes keys back in that order, and leaves out a field the record didn't have while it still holds its default. New items use the order in the table. An unreadable record is skipped when the library opens: falcon lists it in `Library::skipped`, never writes to it, and `save_to` copies its folder unchanged. Checked on four real libraries (3,905 readable records): every record saves back byte for byte.

### Comments (observed in code)

Region notes on images and timed notes on video and audio live in an optional `comments` array on the item. JavaScript adds the key the first time a comment is made, so it comes after the keys an item starts with. None of the fixture's items has comments. Eagle's filter panel calls these "annotations". They are separate from the item's `annotation` string, which is the note.

| Key | Type | Notes |
|---|---|---|
| `id` | string | Same generator as item IDs |
| `x`, `y`, `width`, `height` | number | Image regions only. Pixels of the **original image**, not fractions: Eagle scales the dragged box by `image.width / displayed width`. New boxes are rounded to integers. After a move or resize the values can be fractional. Boxes of 10 px or less in either direction are discarded |
| `duration` | number | Timed notes only. Seconds into playback (`currentTime`, fractional). Eagle re-sorts the list by `duration` after adding one |
| `annotation` | string | Starts `""` for a region. It holds the `innerHTML` of an editable box (`<br>` for line breaks, `&amp;` turned back into `&`) |
| `lastModified` | integer | ms. Set on creation and not bumped when the text, position or size changes |

Every comment change saves the whole item, so the item's own `lastModified` and `mtime.json` move as in any edit. Deleting the last comment leaves `"comments":[]`.

Falcon models these as `model::Comment`. `Item::comments()` and `Item::set_comments()` read and write the key without moving it or any other key in `extra`. On `Library`, `add_comment` fills in the id and time, and keeps timed notes sorted (a new note goes after others at the same time). `update_comment` replaces a comment by id, and `delete_comment` removes one. Falcon re-serialises each comment in Eagle's key order: `id`, region or `duration`, `annotation`, `lastModified`, then unknown keys.

### Manual order (observed in code)

A folder sorted `MANUAL` keeps its hand-arranged order in each item's optional `order` object: `order[<folder id>]` is a **decimal string**, such as `"1789479235636.5"`. JavaScript adds the key the first time an item is dragged, so it comes after the keys the item started with. A new folder id goes at the end of the object.

- **Sorting:** the `MANUAL` sort compares these values **as strings**, largest first. An item with no entry (or an empty one) uses its `modificationTime` turned into a string, so undragged items fall in import order, newest first. `sortIncrease: false` reverses the list. As strings, `"999"` sorts above `"1000"`. This never matters for real timestamps, which all have 13 digits.
- **Dragging:** Eagle works in exact decimals (a big-number library). Divisions round to 20 places, with halves rounded away from zero. Values are written in the shortest form (`toPrecision()` with no argument, so `"2500"`, not `"2500.0"`). Dropping *n* items between neighbours `prev` and `next` sets `middle = (prev + next) / 2` and `step = (middle - next) / n`. The last dragged item gets `middle`, and each earlier one gets `middle + step × k`, which moves towards `prev`. Before the first item, `prev` is the first item's value + 1 (- 1 when descending). After the last item, `next` is the last value - 1 (+ 1 when descending). A neighbour with no entry counts as its `modificationTime`.
- **Collisions:** before placing, Eagle checks whether two of the other items share a value. If they do, every item *i* (counting from 0 in the current view) that has an entry gets `value - i × 10^-(digits in the view's item count + 1)`. The step is a JavaScript float multiplication, float noise included (`0.01 × 57` gives `0.5700000000000001`). Eagle reads the direction flag here before it is set, so the values always step down.
- **Side effects:** dragged items save as an ordinary edit (`lastModified` bumped). A folder that wasn't on `MANUAL` switches to `MANUAL` with `sortIncrease: true`.
- Dropping onto one of the dragged items, or into a view with no other items, throws in Eagle, so nothing changes.

Falcon: `Item::manual_order(folder)` reads the value as a number, `Item::manual_sort_key(folder)` gives the string the sort compares, and `Item::set_manual_order` writes one. `search::sort_manual(items, folder, increase)` is the `MANUAL` sort. `Library::move_items_manually(folder, ids, before)` moves items as a block before another item (`None` for the end), using the same decimal arithmetic (`manual::Dec`) and collision handling. It works out the order over all untrashed items in the folder, where Eagle uses the items currently shown (inferred to be the same when no filter is on). Moving items that aren't in the folder, or placing them before one of themselves, is refused.

## `mtime.json` (observed)

An object that maps item IDs to ms times, ending with `"all"`. In the fixture it lists only 32 of the 91 items, yet `all` is 91. So `all` is the library's total item count, not the number of entries.

Eagle's Empty Cache and Reload reads the library again and rewrites `mtime.json` from what it read. Falcon's `Library::rebuild_mtime()` does the same from the items in memory: one entry per item that has a `lastModified`, in id order, then `all` set to the item count.

## Encoding (observed)

Eagle writes compact JSON with no trailing newline and keeps keys in insertion order.

## How falcon does it

`falcon_library::Library::open` checks the library's version (see [Newer libraries and outside changes](#newer-libraries-and-outside-changes)) and loads every JSON file into typed structs. `Library::save_to` writes them back out.

- **Lossless:** unknown keys are kept in an `extra` map on each struct.
- **Number format:** `ratio` stays a raw `serde_json::Number`, so `45` isn't rewritten as `45.0`.
- **Key order:** each item remembers whether `width` came before `height`.

Together these make every JSON file in the test fixture re-serialize byte-for-byte, which the `round_trips_byte_identically` test checks.

## Newer libraries and outside changes

**Version check.** `metadata.json` carries the `applicationVersion` of the Eagle that last saved it. Falcon reads and writes the format of Eagle 4.0.0 (`library::APP_VERSION`), and `Library::open` refuses a library whose `applicationVersion` is newer, with `Error::NewerLibrary`, as Eagle refuses a library from a newer release and asks for an update. Versions are ordered the way Eagle orders them (`compare_versions`): an optional leading `v`, then up to four dot-separated numbers compared as numbers (`10.0.0` is newer than `4.0.0`, and a missing number counts as 0, so `4` equals `4.0.0`). A `-prerelease` part sorts before the release itself (`4.0.0-beta.2` is older than `4.0.0`, `5.0.0-beta` is still newer than `4.0.0`), its dot-separated parts compare as numbers when both are numbers and as text otherwise, and a shorter list sorts first. A `+build` suffix is ignored. A version that can't be read is not refused.

**Write guard.** `Library::write_metadata_file` is the one place `metadata.json` is written while a library is open. Before writing, it reads the file on disk and refuses, writing nothing, when

- the disk copy's `applicationVersion` is newer than 4.0.0 (`Error::NewerLibrary`), or
- the disk copy's `modificationTime` is later than the one falcon last read or wrote (`Error::SyncConflict`). Another app or a sync tool saved the file in the meantime, and writing would lose its changes.

A disk copy that can't be read or parsed is overwritten. Falcon's own writes set the remembered time, so they never conflict with themselves. A refused write sets a flag that `Library::take_conflict()` returns once, and the app uses it to reload the library from disk (see [Several libraries](app.md#several-libraries)). In batched mode the refusal comes at the flush, and the pending set is dropped rather than retried. Every caller of the library trips the guard, the HTTP API included.

**Picking up outside changes.** Eagle polls `metadata.json` and `mtime.json` every 4 seconds with `fs.watchFile`, and falcon polls the same two files at the same interval (`crates/falcon-library/src/sync.rs`; the app's polling thread is `crates/falcon-app/src/watch.rs`). The thread compares file times and, when one moved, reads and parses that file off the UI thread. A file caught half written doesn't parse and is read again on the next poll. An empty `mtime.json` is ignored, as in Eagle, because it is being rewritten. Applying a change is quick and happens with the library locked, and it never writes, since the changes are already on disk.

- **`metadata.json` changed:** `Library::take_external_metadata` takes it only when its `modificationTime` is later than the one in memory. The folder tree, smart folders, quick access and tag groups it holds replace the ones in memory, including edits still waiting in batched mode, and its time becomes the base later writes are checked against.
- **`mtime.json` changed:** `Library::sync_items` compares it with the stamps in memory. An id falcon doesn't hold is a new item and is read from its `metadata.json`. A larger stamp is an edited item, read again. When the `all` count differs from the number of items held, `images/` is listed to drop items whose folder is gone and to add folders the index missed. An item that can't be read yet, because a sync is still copying it, waits for the next poll. A pending write of an item that was replaced is dropped, since the disk copy is newer.
- **Falcon's own writes** come back through the same path and change nothing, since their stamps are the ones in memory.
- **A vanished library folder** is reported once, and `Library::discard_pending_writes()` drops everything batched mode was holding, so nothing is written to where the library used to be.

## Backups

Eagle keeps snapshots of the library `metadata.json` in `backup/`. Only that one file is backed up; item `metadata.json` files, `mtime.json` and `tags.json` are not.

| Behaviour | Status | Detail |
|---|---|---|
| File name | observed | `backup-YYYY-MM-DD HH.mm.ss.SSS.json` in local time, with dots between the time fields, e.g. `backup-2026-10-26 02.15.25.310.json` (fixture and Eagle's code) |
| Contents | observed | A byte copy of `metadata.json` |
| When | observed | After Eagle rewrites `mtime.json` because something changed (that rewrite is debounced by 1 s after the last change), and on a 30-minute timer while it runs. Not on every `metadata.json` write. There is no preference for it |
| Damaged source | observed | No backup when `metadata.json` doesn't parse or has no truthy `applicationVersion` |
| Unchanged check | observed | Skipped when the newest backup has the same **size** as `metadata.json` (the content isn't compared) |
| Which files count | observed | Any name containing `backup-`; sorted by name, newest first |
| Rotation | observed | At most 100. Eagle trims the list it read before copying to its newest 99, so with the new copy 100 remain. Trimming also runs when the copy was skipped |
| Second copy | observed | Eagle also copies `metadata.json` to `eagle-backup/<hash of library path>.json` in its app-data folder, replacing the file whenever it writes a new backup into an existing `backup/` list (not for the very first one) |
| Recovery | observed | When `metadata.json` exists but can't be read or parsed (or has no `applicationVersion`), Eagle walks the backups newest first, skips empty ones and ones that don't parse or lack a truthy `modificationTime`, keeps the damaged file as `backup/metadata.json.damaged-<ms>`, and copies the first good backup over `metadata.json`. A missing `metadata.json` is reported as a missing library instead |

### How falcon does it

`falcon_library::backup` follows the table. `Library::backup_metadata()` takes a backup now (returning `None` when skipped), `Library::backups()` lists them newest first, and `Library::restore_metadata_from_backup(path)` restores a chosen one. `Library::open_or_restore(path)` opens a library and, when `metadata.json` can't be read or parsed, restores the newest usable backup first, as Eagle does on launch. falcon also requires the backup to load as a `LibraryMetadata`, slightly stricter than Eagle's `modificationTime` check (inferred to be harmless: a backup without the standard keys would break Eagle too).

`Library::save_metadata` and `Library::flush` do not take backups, because Eagle doesn't back up on each write. `FlushReport::wants_backup()` says when a flush wrote `mtime.json` or `metadata.json`. Callers take the backup with `backup_metadata()`, or `backup::backup_library(root)`, which needs only the library folder and so can run on another thread while the library is in use (a `metadata.json` caught mid-write doesn't parse and is skipped). `falcon backup <library>` takes one by hand; the app schedules them (see [Saving](app.md#saving)).

The second copy lives outside the library. `backup::app_data_backup_name(root)` names it `<hash>.json`, where `<hash>` is the 32-bit FNV-1a hash of the library path (no trailing slash) over its UTF-16 code units, as 8 lowercase hex digits, the same name Eagle gives it. The app keeps these in `eagle-backup/` inside its own app data folder, `~/Library/Application Support/Falcon/eagle-backup/` on macOS, and `backup::backup_library_with(root, app_backups)` replaces the library's copy with every new backup. A failure there is ignored, as in Eagle, and the copy is written through a temporary file and a rename so a reader never sees half a file. Nothing is written for a library whose folder is gone, so a backup never recreates a moved or deleted library.

## Merging libraries

Eagle's Library ▸ Import Library merges another library into the open one. Falcon's `falcon_library::library_merge` follows it:

- **Refusals:** the library that is open is refused, a folder without `metadata.json` (or with one that doesn't parse) is not a library, and a library from a newer Eagle is refused as on open.
- **Folders:** a top-level folder whose id the open library already has is not added again, and its subfolders go through the same test inside the existing one. New folders come in with their whole subtree.
- **Smart folders and tag groups:** those whose id is already present are skipped and the rest are appended.
- **Items:** every `images/<id>.info` folder the open library lacks is copied verbatim, and ones it has are left alone. The items keep their folder ids, which the merged tree now contains.

Falcon differs from Eagle in two small ways. It copies only `*.info` folders, not stray files in `images/`. And a new folder's descendants whose ids already exist elsewhere in the open library are merged into those instead of being added a second time, so the tree never holds one id twice. Each item folder is copied under a temporary name and renamed into place, so an interrupted merge never leaves half an item.

The work is split so the app stays responsive. `merge::plan(&lib, source)` reads the other library and reports what would come in (`MergePlan::counts()`). `merge::copy_items` needs no lock on the open library, runs on a background thread, reports progress, can be cancelled and returns the folders copied, with the reason for any that failed. `Library::finish_merge` then merges the folder tree, saves `metadata.json` and registers the copied items in memory and in `mtime.json`, returning a `MergeReport` of folders, smart folders, tag groups and items added.

## Inboard backups

Eagle's File ▸ Import ▸ Inboard reads a backup folder made by the Inboard app, and `falcon_library::inboard` reads it the same way. The folder holds `InboardBackup.json`, with `folders` (each with a `title`) and `items`, plus the files in `images/` and `screenshots/`.

- **Folders:** one top-level folder is created per Inboard folder, all of them and in order. When two share a title, items go to the later one (`inboard::folder_map`).
- **Items:** only items whose `type` is `"image"` are imported. The file is the last component of the item's `path`, looked up in `screenshots/` when that path runs through a `/screenshots/` folder and in `images/` otherwise. An image whose file isn't in the backup is counted as missing (`InboardBackup::missing`) and skipped.
- **What an item keeps:** Inboard's `title` as its name, its `tags`, its `url`, its trashed state (`isTrashed` other than 0, or `true`) and its `date` as the date added. A numeric `date` is read as milliseconds, and a string as RFC 3339 or `YYYY-MM-DD HH:MM:SS ±ZZZZ`.
- **Folders of an item:** its `folders` are Inboard folder titles, mapped to the new folder ids; titles that match no folder are dropped.

`inboard::is_backup(dir)` says whether a folder has the index, and `inboard::read(dir)` returns the folders and importable items. The import itself goes through the ordinary import path, so duplicate handling and thumbnails are the same as for other files.

## Undo

| Behaviour | Status | Detail |
|---|---|---|
| Eagle's undo | observed | Not a history. Some actions (removing items from a folder, deleting a folder or smart folder, renaming a folder, deleting a comment) show a notice with an undo link that holds one restore callback for the notice's duration plus 5 seconds; ⌘Z runs the same callback. Each new notice replaces it |
| How Eagle restores | observed | The callback puts the old values back in memory and saves them like any edit, so restored items go through the normal item save, which stamps a new `lastModified` |
| Permanent deletes | observed | Emptying the trash and deleting permanently remove the `.info` folders and offer no undo |

### How falcon does it

falcon keeps a real history. `Library::begin_transaction(label)` starts recording and `end_transaction()` returns a `undo::Transaction`, or `None` when nothing changed. While a transaction is open, the write paths record what they replace:

- `register_item` (every item save and import) records the item's first pre-edit copy, or its id as created when it didn't exist.
- `save_metadata`, `save_tags` and `save_filters` record the value as last saved, once per transaction. The library keeps a copy of the last saved `metadata`, `tags` and `saved_filters` for this, so edits that change the fields before saving still record the old value.
- `forget_item` (permanent delete, empty trash) marks the transaction `irreversible`. The files are gone, so the app clears its undo history when such a transaction lands, as Eagle offers no undo there either.

`Library::revert(tx)` restores the recorded state in a new transaction and returns it, so reverting that redoes the edit. Items are saved through `update_item`, so a rename moves the original and thumbnail back, and `lastModified` moves forward, which matches Eagle's restore (observed above). Items created in the transaction (imports) go to the trash, with their files kept. Library metadata gets a new `modificationTime`. Recording never changes what is written, so the fixture round trip is unaffected.

## Write batching

| Behaviour | Status | Detail |
|---|---|---|
| Item saves | observed | Queued per item id in a worker queue (8 at a time). A later change to an item still waiting in the queue replaces the queued copy, so each item is written once. `lastModified` is stamped when the file is written |
| `mtime.json` | observed | 2 s after the item queue drains, then after another 1 s debounce, Eagle rewrites `mtime.json` from its in-memory map of id to `lastModified`. It drops ids no longer in the library, adds new ones and sets `all` to the item count. It is written to a temporary file and moved into place |
| Library `metadata.json` | observed | Folder, smart folder, quick access and tag group changes are debounced by 300 ms and written by a one-at-a-time queue |
| Backup | observed | Taken right after a changed `mtime.json` is written (see Backups) |

### How falcon does it

`Library::set_write_mode(WriteMode::Batched)` makes the item, `mtime.json`, `metadata.json`, `tags.json` and `saved-filters.json` writers mark what is dirty instead of writing. `Library::flush()` then writes each dirty item once, `mtime.json` once, and each library-wide file once, and returns a `FlushReport`. Because every writer serialises the whole in-memory value, the bytes after a flush are the ones immediate mode would have written last. When `mtime.json` or `metadata.json` was written, `FlushReport::wants_backup()` is true, and the app schedules a backup, as Eagle backs up after rewriting `mtime.json` (falcon also counts a metadata write, inferred harmless: the backup is skipped when the size is unchanged). The flush itself takes no backup. File renames and permanent deletes still happen at once. An item deleted before the flush is skipped. If a write fails, the pending set is kept for the next flush. Dropping a batched library with pending writes flushes it, ignoring errors. `WriteMode::Immediate` is the default, used by the CLI, the server and the tests. Switching back to it flushes first.

falcon writes JSON files in place rather than through a temporary file, as before.

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
| Smart folders | observed | Exporting a smart folder writes a folder pack of its matching untrashed items. `folder` is the smart folder copied with the folder keys above under a new id, with no `children` (subfolders are not carried), and every item's `folders` is set to that new id, so the items import into one plain folder. Smart folder rules are never carried |

### How falcon does it

The app writes packs on a background thread, so the work is split in two: `plan_pack(&lib, source)` builds `pack.json` and the list of item folders into a `PackPlan` that holds no reference to the library, and `write_pack(&plan, dest, on_item)` writes it, reporting each item folder added. `falcon_library::pack::export_pack(&lib, PackSource::Items(ids) | PackSource::Folder(id) | PackSource::Smart(id), dest)` does both and writes a pack following the table, using the `zip` crate with stored entries and each file's time and Unix mode. `preview_pack(&lib, pack)` reads a pack without extracting it, as Eagle does before it asks: it opens the zip, adds up the uncompressed size of every entry, parses `pack.json` and counts how many of its items look like untrashed items already in the library, using the same duplicate key as `skip_duplicates`. It returns a `PackPreview` of `items`, `duplicates` and `bytes`, and fails with `Error::Pack` when the file isn't a zip or `pack.json` is missing or has no `images` list. `import_pack(&mut lib, pack, &PackImportOptions { folder, skip_duplicates })` imports one and returns the new item ids. Item files are moved in as they are (thumbnail included), not re-imported, as Eagle does. Differences from Eagle, all inferred to be harmless:

- falcon drops item `folders` ids that don't exist after import, where Eagle would write `null` or keep a dangling id.
- An item whose `<ID>.info` folder is missing from the pack is skipped; Eagle still lists it, with no files.
- A record without `isDeleted` (an exported trashed item) is written back with `"isDeleted": false`, since falcon's item model requires the key.
- The duplicate key covers images and videos only; audio and font keys are not implemented.

The CLI has `falcon export <library> <out.eaglepack> [--folder ID | --smart ID | ITEM...]` and `falcon import-pack <library> <pack> [--folder ID] [--skip-duplicates]`.
