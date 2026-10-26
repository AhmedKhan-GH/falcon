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
