# Import pipeline

## What Eagle does

Every claim is from Eagle's plain-JS background window code (`processImage` in `background.js`). The Electron main process is V8 bytecode and was not read.

1. **Stat the source (observed).** `size` is the byte length. `btime` and `mtime` are the file's birth and modification times as integer milliseconds, read before the copy.
2. **Reject (observed).** An unknown extension is refused. So is an empty file, unless its extension is `txt`.
3. **Name (observed).** The extension is removed, the name is sanitised and trimmed, shortened to fit the file system, normalised to NFC, and replaced by `_` if empty. The upload step strips `/` and `%` first.
4. **Directory (observed).** `images/<ID>.info/` is created. The original is `<name>.<ext>` and the thumbnail `<name>_thumbnail.png`.
5. **Item record (observed).** `isDeleted` is false, `url` and `annotation` are empty, `tags` and `folders` come from the caller.
6. **Copy (observed).** `copyFile`, then `utimes` so the copy keeps the source's mtime. A failed copy removes the `.info` folder and reports an error.
7. **Thumbnail (observed).** Sets `width`, `height`, `noThumbnail` and `removeThumbnail`. See [Thumbnails](thumbnails.md). If it throws, the item is still added, without a thumbnail.
8. **Palette (observed).** Computed from the thumbnail, or from the original when `noThumbnail && !removeThumbnail`. When `removeThumbnail` is set, the thumbnail is deleted once the palette task finishes.
9. **Metadata write (observed).** Each write sets `lastModified` to the current time and writes the item's `metadata.json`.

### Timestamps

- `btime` and `mtime` are the file's own times (observed).
- `modificationTime` is the import time. A batch gives each item `Date.now() + index`, and a repeated value is incremented, so values within a batch are unique (observed). Later saves do not change it (observed).
- `lastModified` is the time of the latest `metadata.json` write, so it is a little later than `modificationTime` (observed).

### mtime.json

- Maps each item id to its `lastModified`, followed by `all`, the number of items loaded from `images/` (observed). Trashed items are probably included (inferred; not checked against a library with trash).
- It is rewritten about one to two seconds after the write queues drain, not once per item (observed).
- Other Eagle instances watch it to notice new or edited items (inferred from the reader code).

## How falcon does it

`Library::import_file(path, &ImportOptions)` runs the same steps in the same order and returns the new id. `ImportOptions` holds `folders`, `tags`, `palette` (default true), `annotation` and `url`.

- **Source times.** `btime` falls back to the mtime when the file system has no birth time. Eagle's behaviour in that case is unknown.
- **Name.** Falcon removes control characters and `/ ? < > \ : * | " %`, trims spaces and trailing dots, normalises to NFC, and cuts the name so that `<name>_thumbnail.png` fits in 255 bytes. The exact length Eagle allows is not known.
- **Copy.** `fs::copy`, then the source's access and modification times are set on the copy. There is no cut mode and no fallback copy.
- **Unsupported formats.** Only png, jpg, webp, gif, bmp and svg are decoded. Any other extension is imported with `width` and `height` of 0, no thumbnail and no palette. Eagle would generate a thumbnail for many of them.
- **Failures.** If decoding or encoding fails, the item is kept with no thumbnail and no flags, as Eagle does for a thrown error. If the copy fails, the `.info` folder is removed and the error is returned. An unknown folder id is rejected before anything is written.
- **Palette.** Taken from the decoded thumbnail, so it sees the same WebP artefacts Eagle's analyser sees. `palette: false` skips it.
- **Timestamps.** `modificationTime` is the later of the current time and the previous import's value plus 1 ms, tracked on the `Library`, so it is strictly increasing across calls. `lastModified` is read after the palette is done.
- **mtime.json.** Updated and written after every import, not debounced. The entry is set to `lastModified`, and `all` becomes the count of loaded items. Existing entries that are missing from the file are left missing.
- **Not done.** Library `metadata.json` is not touched, no backup is taken, and IPTC keywords, stars and concurrency are not implemented.
