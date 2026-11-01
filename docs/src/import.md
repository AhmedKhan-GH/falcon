# Import pipeline

## What Eagle does

Every claim is from Eagle's plain-JS background window code (`processImage` in `background.js`). The Electron main process is V8 bytecode and was not read.

1. **Stat the source (observed).** `size` is the byte length. `btime` and `mtime` are the file's birth and modification times as integer milliseconds, read before the copy.
2. **Reject (observed).** A file with no extension is refused ("File format is not recognized"), and so are unfinished browser downloads (`.crdownload`, `.part`, `.download`), which `getExt` treats as having none. Any other extension, known or not, goes ahead. So is an empty file refused, unless its extension is `txt`. The refusal goes to the failed-imports list; there is no dialog.
3. **Name (observed).** The extension is removed, the name is sanitised and trimmed, shortened to fit the file system, normalised to NFC, and replaced by `_` if empty. The upload step strips `/` and `%` first.
4. **Directory (observed).** `images/<ID>.info/` is created. The original is `<name>.<ext>` and the thumbnail `<name>_thumbnail.png`.
5. **Item record (observed).** `isDeleted` is false, `url` and `annotation` are empty, `tags` and `folders` come from the caller.
6. **Copy (observed).** `copyFile`, then `utimes` so the copy keeps the source's mtime. A failed copy removes the `.info` folder and reports an error.
7. **Thumbnail (observed).** Sets `width`, `height`, `noThumbnail` and `removeThumbnail`. See [Thumbnails](thumbnails.md). If it throws, the item is still added, without a thumbnail.
8. **Palette (observed).** Computed from the thumbnail, or from the original when `noThumbnail && !removeThumbnail`. When `removeThumbnail` is set, the thumbnail is deleted once the palette task finishes.
9. **Metadata write (observed).** Each write sets `lastModified` to the current time and writes the item's `metadata.json`.

### Format metadata

Eagle's thumbnail modules (`app/js/thumbs/`) also add keys for some formats. New keys go after the ones already set, in the order below. In Eagle they land before `lastModified` and `palettes`. Falcon keeps unknown keys after `palettes`, so it writes them at the end. Eagle's reader doesn't depend on key order.

**RAW photos (observed).** These are 3fr, arw, orf, mrw, nef, nrw, raw, cr2, crw, dng, erf, pef, raf, rw2, sr2, srw, x3f and cr3. Eagle runs dcraw's identify on the file and stores `rawMetas`:

| Key | Value | Example |
|---|---|---|
| `timestamp` | date taken, ms; dcraw prints local time and Eagle parses it back as local | `1541495719000` |
| `camera` | make and model as dcraw names them | `"Pentax K-1 Mark II"` |
| `isoSpeed` | integer | `200` |
| `focalLength` | string | `"200.0 mm"` |
| `shutter` | string, `1/N.N sec` under a second, else `N.N sec` | `"1/320.0 sec"` |
| `aperture` | string | `"f/3.2"` |
| `width`, `height` | dcraw's full sensor size, also copied to the item | `7392` |

- Missing values are left out. An empty object is not stored.
- JPEG, HEIC and TIFF get no `rawMetas`. Eagle reads their EXIF only for orientation.
- Smart-folder rules parse the strings back: the first number of `aperture` and `focalLength`, and the second number of `shutter`. So `"2.0 sec"` never matches a shutter rule.

**Video (observed).** `resolutionWidth` and `resolutionHeight` (copies of `width` and `height`), then `duration`. All come from Chromium's `<video>` element. `duration` is in seconds with a fraction.

**Audio (observed).** mp3, wav, flac, ogg, aac and m4a. `width` 280 and `height` 140 (the waveform thumbnail, see [Thumbnails](thumbnails.md)), then `duration` from an `<audio>` element. Then `bpm` (observed in `js/thumbs/audio.js`): after drawing the waveform, Eagle mixes the decoded audio to mono and runs a bundled beat tracker (music-tempo) over it. For files over 3 minutes it analyses only the start: the first 3 minutes, or the first 2 past 10 minutes, or the first 1 past an hour. The tracker searches beat intervals of 0.3 to 1 second (60 to 200 BPM) and reports the tempo as a string with three decimals. Eagle stores it with `parseFloat`, so it is a JSON number such as `119.985`, or `120` when whole. When the tracker finds no tempo, nothing is stored (inferred for silence, where the tracker has no beats to follow). Refresh Thumbnail recomputes it.

**Fonts (observed).** For ttf, otf, ttc, otc, woff and woff2, Eagle parses the file with opentype.js and stores `fontMetas`:

- First, one key per `name` table entry, in table order: `copyright`, `fontFamily`, `fontSubfamily`, `uniqueID`, `fullName`, `version`, `postScriptName`, `trademark`, `manufacturer`, `designer`, `description`, `manufacturerURL`, `designerURL`, `license`, `licenseURL`, `preferredFamily`, `preferredSubfamily`, and others. Each is a map from language code (`en`, `zh`, `zh-TW`, `ja`, …) to text. A Windows record replaces the Macintosh text for the same language but keeps its position.
- Then `numGlyphs`.
- Then `support`, which holds `zh_CN`, `jp`, `zh_TW` and `kr` set to true when the font has glyphs for a few test characters of that script.
- Then `weight`. It is the first of Thin, Hairline, ExtraLight, UltraLight, Light, Regular, Normal (written Regular), Medium, DemiBold, SemiBold, Bold, ExtraBold, UltraBold, Black, ExtraBlack and Heavy found in the lower-cased PostScript name. If none is found, the subfamily is used when it is one of those words, and otherwise Regular. " Italic" is appended when the PostScript name contains "italic". Because "bold" is tested before "extrabold", an ExtraBold font is labelled Bold.
- A font with no names gets a random `postScriptName` and `fullName`. One with no PostScript name uses its `uniqueID`.
- With the "auto tag" font preference, which is on by default, the weight is also pushed onto `tags`.
- Fonts opentype.js can't parse (collections, woff2) go through a second parser, Typr, which writes a different fixed set of keys, every one with only an `en` value.
- **WOFF 2 in Eagle 4.0 (observed in code, result inferred):** woff2 is routed to the font handler, but neither the bundled opentype.js 0.11 nor its Typr build has a WOFF 2 reader (neither looks for the `wOF2` signature). Both parses are inferred to fail, so a woff2 item gets no `fontMetas` and no weight tag.

### IPTC keywords (observed)

- The preference `general.IPTC` is off by default. When it is on, Eagle reads keywords from files whose extension is exactly `jpg`. A `.jpeg` file is not read.
- It looks only at the first APP13 segment, in the "Photoshop 3.0" resource 0x0404, and takes IIM dataset 25 (keywords) from it. XMP is never read.
- A keyword field over 200 characters is skipped. The rest are split on `;`.
- The parts are appended to the caller's tags, and the whole list is de-duplicated (first occurrence wins).
- This happens before the copy, so the keyword tags come before the font weight tag.

### Duplicates (observed)

Eagle stores no content hash in item JSON and never hashes file bytes. Its duplicate check (`getHashID`) builds a key from metadata alone:

| Kind | Key built from |
|---|---|
| Video | size and duration |
| Audio | size, duration, extension, and `bpm` when known |
| PDF | size, width and height |
| Fonts | size, `fontMetas.uniqueID`, `postScriptName` and `version` |
| URL bookmarks | the video id or the URL |
| Images | the first palette colour rounded up to tens, the second and third colours, then size, width, height and extension |

- svg, tif, tiff and trashed items are skipped.
- Images without a palette fall back to size, width, height and extension.

The key is used when files are imported (seen in `file-uploaded`, `isDuplicateImage` and the `duplicateModal` directive):

- Each imported item whose key matches an item not in the Trash has the first 4,096,000 bytes of both files compared. When they are equal the new item, already written to the library, is held back from the list. The key alone (bmp, tga, txt and formats Eagle can't preview get an empty key) never counts.
- When the upload queue has finished, the Duplicate File Alert opens. It shows the existing item ("Existing", with its first folder or Uncategorized) beside the new one, two radio choices, Use existing file (the default) and Keep both, an Apply all (N) box when more than one file is held, and an Import button. The close button or Esc is Don't Import. ⌘↩ and ⌘Esc answer for every file. A click outside does nothing.
- **Use existing file:** the new item is deleted for good. Each of its folders the existing item isn't in is added to it, with that folder's auto-tags; folders and tags are de-duplicated, and the existing item's `modificationTime` becomes the new item's, so it sorts as just added. The new item's own tags and notes are not carried over.
- **Keep both:** the new item stays.
- **Don't Import:** the new item is deleted for good.
- The preference `notification.notification.when.repeatImage` (Notify me when importing duplicates, under Notification) is on by default and also needs the notification master switch. When either is off, nothing is asked and duplicates are imported as new items.

### Folder auto-tags (seen)

A folder's `tags` key holds its auto-tags. Eagle's code calls the list it applies `extendTags`; it is worked out at run time (`getExtendTags`) and deleted before `metadata.json` is written, so it is not a setting.

- The list is the folder's own tags, then its parent's, then the grandparent's and so on to the top level. Only the first copy of each tag is kept, and the result is reversed. A folder with tags `[d, e]` under `[c, b]` under `[a, b]` applies `[a, b, c, e, d]`.
- Importing or pasting files into a folder in Eagle's window appends the list to the item's tags and removes repeats. Importing into several folders (the add-from-URLs path) joins each folder's list in turn.
- Adding items to a folder (the inspector's folder picker, Add to Folder) appends the tags an item lacks, but only when the item was not already in that folder.
- Dropping items onto a folder appends the missing tags to every dropped item, even one already in the folder.
- Eagle's API `addFromPath` and `addFromPaths` don't apply auto-tags; `addFromURL(s)` do.
- There is no preference for any of this.

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
- **Unsupported formats.** As in Eagle, `import::is_recognized` refuses only files with no extension and unfinished browser downloads (`.crdownload`, `.part`, `.download`), with `FailKind::Unrecognized`, so they appear in the failed-imports list. Every other file imports. Formats falcon can't draw itself go to QuickLook, and a file nothing can draw is imported with `width` and `height` of 0, no thumbnail and no palette. Eagle marks such an item `noPreview: true` and leaves out `width`, `height` and `palettes` (see [Library format](library-format.md)); falcon does not write those differences yet.

  Eagle's "The file format is not supported" dialog (`Dialog.NotSupport.Format.*`) appears only when something dropped from a browser is neither a file nor a link nor image data (`onDropContainer`). Two other uses of it, for unknown file types in drops and folder imports, are commented out in Eagle 4.0.0, and those files are imported. falcon only receives file drops, so it has no such dialog.
- **Failures.** If decoding or encoding fails, the item is kept with no thumbnail and no flags, as Eagle does for a thrown error. If the copy fails, the `.info` folder is removed and the error is returned. An unknown folder id is rejected before anything is written.
- **Link files.** A dropped `.url` (Windows shortcut) or `.webloc` (macOS property list, XML or binary) file takes its address as the item's `url` (`weblink::read`), unless `ImportOptions::url` is set. Files over 1 MiB or without an address leave `url` empty.
- **Palette.** Taken from the decoded thumbnail, so it sees the same WebP artefacts Eagle's analyser sees. `palette: false` skips it.
- **Timestamps.** `modificationTime` is the later of the current time and the previous import's value plus 1 ms, tracked on the `Library`, so it is strictly increasing across calls. `lastModified` is read after the palette is done.
- **mtime.json.** Updated and written after every import, not debounced. The entry is set to `lastModified`, and `all` becomes the count of loaded items. Existing entries that are missing from the file are left missing.
- **Folder auto-tags.** With `ImportOptions::folder_auto_tags` on, `commit_import` appends the auto-tags of the item's folders (`autotag::auto_tags_for`) to its tags, skipping tags it already has, in the same write. It is off by default, as for Eagle's `addFromPath`; the app turns it on for every import (drops, ⌘V, the collect window). The list is computed from the folders' `tags` by `autotag::folder_auto_tags`, which follows Eagle's order exactly; a cached `extendTags` key in a file is ignored.
- **Not done.** Library `metadata.json` is not touched, no backup is taken, and stars and concurrency are not implemented.

### Format metadata in falcon

- **`rawMetas`.** Falcon reads EXIF with the `kamadak-exif` crate instead of running dcraw, and prints the values the way dcraw does:
  - `camera` is the Make tag shortened to a known maker name (`NIKON CORPORATION` becomes `Nikon`), followed by the Model with a repeated maker word removed. This is an approximation of dcraw's table (inferred).
  - `timestamp` is DateTimeOriginal, or DateTime, read as local time.
  - `width` and `height` are not written. EXIF doesn't hold dcraw's full sensor size.
  - Readable RAW files: the TIFF-based ones (dng, nef, nrw, cr2, arw, srw, pef, 3fr, erf), plus four with their own containers. crw, mrw and x3f give nothing.
    - **orf, rw2:** TIFF with another magic number in place of 42 (`IIRO`, `IIRS` or `MMOR` for Olympus, `IIU\0` for Panasonic). Falcon reads them as TIFF. The camera tags sit where they do in any TIFF.
    - **raf:** a `FUJIFILMCCD-RAW` header with the embedded JPEG preview's offset and length (big-endian, at bytes 84 and 88). That JPEG's EXIF holds the camera facts.
    - **cr3:** an ISO media file (`ftyp` brand `crx `). Inside `moov`, Canon's `uuid` box (`85c0b687-820f-11e0-8111-f4ce462b6a48`) holds `CMT1`, a TIFF block with the main IFD (Make, Model, DateTime), and `CMT2`, a TIFF block whose first IFD holds the EXIF tags (exposure, f-number, ISO, DateTimeOriginal, focal length). Falcon reads both.
    - All four produce the same `rawMetas` keys and formats as the TIFF-based files.
    - Eagle's dcraw build is inferred to read orf, rw2 and raf, which classic dcraw supports. Classic dcraw predates CR3, so Eagle is inferred to store no `rawMetas` for cr3 files, where falcon does.
  - `ImportOptions::camera_metadata_all_photos` (off by default) also stores `rawMetas` for jpg, jpeg, tif, tiff, heic, heif, png and webp. Eagle never does this. It lets the camera filter work on ordinary photos.
- **`duration`.**
  - Audio is read with symphonia, from the frame count, or by adding up packet durations when the header has no count.
  - mp4, mov, m4v, 3gp and f4v are read from the `mvhd` box (duration ÷ timescale).
  - Other video formats fall back to Spotlight (`mdls`), which knows nothing about files in temporary folders.
  - Values can differ slightly from a browser's, for example around MP3 encoder padding.
- **Video sizes.** `resolutionWidth` and `resolutionHeight` copy the item's `width` and `height`.
- **`bpm`.** `tempo::bpm(path, ext)` estimates the tempo with falcon's own method rather than Eagle's beat tracker, over the same opening stretch (`tempo::analysed_seconds`):
  - The audio is decoded with symphonia and mixed to mono. A loudness-rise envelope is built at about 100 frames a second: the log RMS over two hops, its positive differences, lightly smoothed, with the mean removed.
  - The envelope's autocorrelation is scored for every tempo from 60 to 200 BPM in 0.01 steps. Each score averages the autocorrelation at 1 to 4 beat periods, read between lags by cubic interpolation, and is weighted by a gentle preference for tempos near 120 (one octave wide). The best score wins, rounded to three decimals, and is written right after `duration`, whole values without a fraction.
  - Nothing is written when the file is silent, too short (under about two seconds), has no clear rises (a steady tone), or no beat stands out (hiss).
  - Checked on generated clicks at 60, 75, 90, 120, 140 and 174 BPM, and on a drum loop over a drone, all within 1 BPM. Clicks at 200 read as 100, an octave down, because the preference for 120 breaks the tie. Eagle's tracker can also land on a different octave, and values on real music will differ from Eagle's.
  - Refresh Thumbnail recomputes `bpm` for audio items, as Eagle's refresh does (`generated.bpm` replaces the item's value, seen in `app.bundle.js`). An existing key keeps its place in the record; when no beat is found the key is removed, as Eagle's `undefined` drops it on write. Refreshing an item Eagle analysed replaces Eagle's tempo with falcon's estimate, which can differ.
- **`fontMetas`.**
  - Read with `ttf-parser` (ttf, otf, the first face of ttc or otc) after falcon's own WOFF 1 and WOFF 2 unpackers (`fontmeta::sfnt`).
  - WOFF 2 goes further than Eagle: falcon writes the same opentype.js-shaped `fontMetas` (and weight tag) as for the plain font. The Brotli stream is decoded with `brotli-decompressor`, and the tables are rebuilt into an sfnt. Tables stored with a WOFF 2 transform (the packed `glyf` and `loca`, and a transformed `hmtx`) are left out, since names, `maxp` and `cmap` are all metadata needs. WOFF 2 collections are not read.
  - A TrueType woff2 with packed glyphs therefore has no outlines for the 600×600 specimen. Its thumbnail falls back to QuickLook. CFF-flavoured woff2 and woff2 files with plain `glyf` get the specimen.
  - Language codes cover the common Windows and Mac Roman languages. Records in other languages or Mac encodings are skipped, where opentype.js would decode them.
  - Collections get the opentype.js shape, not Typr's.
  - Eagle's MD5 fallback for fonts with a family name but no English PostScript name is not reproduced.
  - `ImportOptions::font_weight_tag` (default true, as in Eagle) adds the weight tag. The app sets it from the Preferences switch in the Preview pane.

### Duplicates in falcon

The `duplicates` module compares file contents instead of copying Eagle's metadata key, and writes nothing to the library.

- **Exact copies.** `find_exact_duplicates` groups items, trash excluded, whose originals hash to the same BLAKE3 digest. Only files that share their size with another file are hashed.
- **Near copies.** `find_near_duplicates(lib, threshold)` takes a 64-bit perceptual hash of each item's thumbnail, or of the original for small images shown without one. The hash is a 32×32 grey DCT, keeping the 8×8 lowest frequencies, each compared with their median. Items within `threshold` bits are grouped, transitively. About 10 suits resized or recompressed copies.
- **Cache.** The `_cached` variants take a `HashCache`, keyed by item id and invalidated when the original's size or mtime changes. `HashCache::save` and `load` keep it as JSON wherever the caller likes.
- **Results.** Groups list item ids in library order.

### The duplicate check on import in falcon

`import_check` holds the library side. `Library::duplicate_candidates(size, ext)` lists items not in the Trash with the same size and extension (no file is read), and `import_check::same_file` compares a staged copy with them by BLAKE3 hash of the whole file, which is at least as strict as Eagle's 4 MB prefix compare. svg, tif, tiff, bmp, tga, txt and files without an extension are not checked. `Staged::original` gives the staged copy's path. `Library::use_existing_for_import(existing, folders)` is Use existing file: the item joins each folder it isn't in, with the folder's auto-tags, and its `modificationTime` becomes the import time. Unlike Eagle, falcon checks before committing, so a duplicate is never written to the library and nothing has to be deleted afterwards. The app's side (holding duplicates and asking) is described in [The desktop app](app.md).

### Keywords in falcon

`ImportOptions::keywords_as_tags` is off by default, as in Eagle. When it is on, the `keywords` module reads:

- jpg and jpeg: the IPTC block in APP13, parsed as proper IIM records (record 2, dataset 25), and the XMP packet in APP1.
- png: the XMP packet in the `iTXt` chunk `XML:com.adobe.xmp`, compressed or not.
- tif and tiff: the IPTC (tag 33723) and XMP (tag 700) entries of the first IFD.

IPTC keywords come first, then the XMP `dc:subject` items. Eagle's 200-character limit and `;` split apply to both. Falcon also drops empty parts, where Eagle would keep an empty tag. Reading `.jpeg`, XMP, PNG and TIFF goes beyond what Eagle does.

## Screenshots

### What Eagle does (seen in its shipped source)

A screenshot is captured to a temporary PNG, converted to the format and quality in Preferences ▸ Screenshot (`screencapture.format`, `screencapture.quality`), and halved when `screencapture.useRetina` is off on a scale-2 screen. The item is named "Screenshot - YYYY-MM-DD HH.mm.ss". It gets the tag "Screenshot" when it has no tags and `screencapture.autoTagging.enable` is on, and is copied to the clipboard when `screencapture.autoWriteClipboard` is on. An area or window screenshot opens the collect window first when `general.showCaptureCollectModal` is on, and the item is saved into the folders chosen there. Otherwise, and always for a full-screen screenshot, it is imported into the library without a folder.

### How falcon does it

`crates/falcon-app/src/capture.rs` makes the file and `app/system.rs` hands it to the import (see Screen capture in the app chapter).

- **Name.** "Screenshot - YYYY-MM-DD HH.mm.ss" in local time, passed as `ImportOptions::name`. The staged file has the same name, with " 2", " 3" and so on added if two shots share a second, but the item keeps the name above.
- **Tag.** "Screenshot" is put in `ImportOptions::tags` unless Preferences ▸ Screenshot turns tagging off. falcon adds it through the same tag helper the collect window uses, so it never doubles a tag typed there. (Eagle adds it only when the item has no tags; a screenshot has none at this point, so the result is the same.)
- **Format and size.** The PNG from `screencapture` is converted with `export::convert_bytes` to PNG, JPG or WebP at quality 90, 80 or 70. A PNG at full size is only renamed. When "Keep the full pixel size" is off and the screen's scale is 1.5 or more (Eagle checks for exactly 2), the longer side is halved first.
- **Where it goes.** The library root with no folders, so folder auto-tags have nothing to add. With "Choose where each screenshot is saved" on, an area or window screenshot waits in the collect window, and Save imports it into the folders picked there with the name and tags shown there. A full-screen screenshot never waits.
- **Temp files.** The file is written to `falcon-paste` in the system temp folder. The import deletes it once it has copied it into the library, and Discard in the collect window deletes it. A cancelled capture leaves no file, a failed conversion removes the raw PNG, and a screenshot that arrives after the library closed is deleted.
- **Same pipeline.** It is an ordinary background import: the duplicate check asks what to do when the picture is already in the library, and a failure goes to the failed-imports list. A batch that held only screenshots imported straight in does not post "Import finished", because the screenshot has its own notification; a screenshot saved from the collect window goes through the normal collect import and does post it.

## Importing folders

### What Eagle does (observed in `app.bundle.js`)

Where a folder from Finder is dropped decides what happens:

| Drop target | Eagle handler | Result |
|---|---|---|
| The item grid | `onDropContainer` | Files, and every file inside dropped folders (walked flat), go into the open folder, or into no folder from All and the other views. No folders are created |
| A folder row in the sidebar | `onDropFolder` | Dropped files go into that folder. Dropped folders are ignored |
| The rest of the sidebar | `onDropSidebar` | Each dropped folder (sorted by name, numbers by value) goes through `uploadFolderToSidebar` with no parent, so its tree lands at the top level. Loose files are ignored |

`uploadFolderToSidebar` walks the folder (`getFolderList`), then creates an Eagle folder named after the directory, with one child folder per subdirectory, children sorted by name with numbers compared by value. The new folders get no icon, tags or description. Each file is imported into the folder that mirrors its directory, and the new top folder is opened.

File ▸ Import Folders (`importFolders`) opens a dialog that picks several folders and sends each through `uploadFolderToSidebar` at the top level. The Import Folders button on an empty folder's page passes that folder as the parent instead, and the one on the empty All page passes none. A picked `.library` is opened instead of imported, but only when it is the only folder picked; otherwise it is skipped. Eagle has no Import Files menu item.

Limits (`uploadFolderToSidebar`, `getFolderList`, `treeDepth`): the walked folder counts as level 1 and each level of subfolders adds one. A folder 15 or more levels deep, or with 100,000 or more files, is refused with a notice and nothing is imported. A folder with 2,000 or more files asks for confirmation first ("contains N items"). Eagle's count leaves out the files directly inside the dropped folder, counting only those in its subfolders (observed in `treeDepth`). The grid drop has no limits. Only the Import Folders dialog skips Pixave libraries (`.pxvlibrary`); a sidebar drop would walk one. Inside folders, junk files (`.DS_Store`, `Thumbs.db` and the like) are skipped, and Keynote, Pixelmator and MindNode packages (`.key`, `.pxd`, `.mindnode`) count as files rather than folders to walk into.

### How falcon does it

`folder_import::walk(dir)` reads a directory into a `Tree` (files and subfolders in natural name order). Hidden files (names starting with a dot) and junk (`Thumbs.db`, `Desktop.ini`, `Icon\r` and the like) are skipped, the three package types count as files, and symbolic links to folders are not followed, so a link cycle can't loop. `folder_import::plan(&tree, parent, new_id)` maps the tree to `NewFolder`s (parents first, siblings in order) and `(file, folder id)` pairs, and `Library::create_folders` adds the folders in one `metadata.json` write. Folder names are the directory names as the file system gives them, as in Eagle. `Tree::stats()` counts levels and files and `folder_import::verdict` applies Eagle's limits (`MAX_DEPTH` 15, `MAX_FILES` 100,000, `WARN_FILES` 2,000). falcon counts every file, including those directly in the dropped folder, so its count can be a little higher than Eagle's.

The app follows Eagle's three drop targets. Because macOS sends no mouse moves while a drag from another app is over the window, the drop position is read from the system mouse location (`drop_pos.rs`) and compared with the sidebar's rect and visible folder rows from the last frame. Dropping only files on the empty part of the sidebar shows a hint instead of doing nothing silently. Each folder imported with its tree is checked against the limits on its own: a refused folder gets a notice, and a large one waits for Import or Cancel in a dialog (one dialog per folder, the others still import). Pixave libraries are skipped by every folder import, the grid drop included, with a message. The grid drop walks folders with the same walk as the tree import, so it skips junk and can't loop through a folder link; it has no size limits, as in Eagle. A `.library` among the dropped paths is opened wherever it lands. Import Folders… (in the library switcher menu, and on the empty All and folder pages, as in Eagle) runs the same import from an `rfd` folder dialog that picks several folders; `App::import_folders_dialog()` is the entry point for a future File menu. Import Files…, next to it, picks files and imports them into the open folder like a grid drop; it is falcon's own. Both dialogs open once the frame is done, after the library lock is released. A package can't be imported by falcon yet (it isn't a regular file), so it is reported as a failed import.

## Failed imports

### What Eagle does (observed in `index.html`, `app.bundle.js` and `background.js`)

A file that fails in `processImage` (no extension, empty, a failed copy and so on) is reported as an `ADD_ERROR` with the error's message as the reason and added to `errorList`. The list survives later imports until it is cleared. While it has entries, a toast at the top centre of the window reads "N files failed to import. Click here for details." Clicking it opens the error modal; its × asks to clear the list. The modal is a table of Name, Source (the file's path; clicking it reveals the file) and Error Description, with a remove button per row, and four buttons: Clear All (after a confirmation that it can't be undone), Copy to Clipboard (the files' paths, put on the clipboard as files), Retry All (every file is imported again with its folders and tags, the list empties and the modal closes) and Close.

### How falcon does it

`import::Failure::new(path, &error)` classifies an error from `stage`, `check_folders` or `commit_import` into a `FailKind` (missing, not a regular file, empty, format not recognised, unreadable, not enough disk space, staging copy failed, library write failed, unknown folder, other) and keeps the full error text as `detail`. `FailKind::describe` gives the reason the app shows. The app's import thread records a `Failure` with the `ImportOptions` used for each file that fails; the list is kept per library until cleared. The "Added N, M failed" message still flashes when a batch ends, and a toast under the toolbar stays while the list has entries, with Eagle's click and × behaviour. The panel lists the file name, its folder (click to show the file in Finder), the reason (the full error on hover) and a remove button per row, with Clear All (confirmed), Copy to Clipboard (the files, ready to paste in Finder), Retry All (with each file's own folders and tags) and Close.

## Auto-import

### What Eagle does (seen in `js/auto-import/auto-import.js`, `background.js`, `preferences.js` and `my_modules/is-network-drive`)

With Preferences ▸ Auto-Import on and a folder chosen (`autoImport.enable`, `autoImport.path`, off and empty by default), Eagle watches `<chosen>/<library name>` (the library's folder name without `.library`), making that folder if it is missing, so one chosen folder serves every library. Watching starts when a library loads and when the setting or folder changes, and stops when the library closes or the setting goes off. If the chosen folder no longer exists, an error dialog says so and nothing is watched; the preferences window also forgets such a folder and turns auto-import off. Choosing a folder on a network drive is refused with a warning: the drive check asks for the file system type and refuses `smbfs`, `afpfs` and `nfs`, and `smb://` paths.

`fs.watch` on the folder (debounced by 500 ms), and one pass 2.5 s after watching starts, list the folder. Each entry is skipped when its name contains `.tmp` anywhere, when it is a folder, when it has no extension or is a `.url` file, when its format isn't one Eagle supports (or a plugin thumbnails), or when it is 1,000 bytes or smaller (SVGs: 1 byte or smaller). A file being handled is locked by name. Eagle then checks it every 500 ms until its mtime has stayed the same for six checks in a row (about three seconds), so a copy in progress is waited for, and imports it like a dropped file: into no folder, with no tags, its name without `%` and emoji. Once the item is added (with or without a thumbnail), the file is deleted from the watched folder. If any files were found, a notice ("N files will start to import") follows 1.5 s later, when notifications are on and `notification.notification.when.autoImport` is on (the default).

### How falcon does it

`crates/falcon-app/src/auto_import.rs` holds the rules and the watcher; the pane is `prefs::auto_import` and the app side `import_ui::auto_import_tick`.

- The same settings and defaults, the same `<chosen>/<library>` folder, watched for the library shown (falcon can have several open; the watcher follows the one on screen). The watcher starts when that library is shown with auto-import on, moves when another library is shown, and stops when the library closes or the setting is turned off. It never starts under the test harness.
- A chosen folder that is gone or on a network volume is refused when picked, and, if it happens later, auto-import pauses with a notice (once, until the preferences change). falcon asks macOS whether the volume is local (`NSURLVolumeIsLocalKey`), which refuses SMB, AFP and NFS as Eagle does and also WebDAV and other remote file systems.
- falcon doesn't make `<chosen>/<library>` by itself; the pane says when it is missing and its Make It button makes it.
- Deleting imported files is a preference, "Delete files after importing them" (`Prefs::auto_import_delete`), on by default to match Eagle. Eagle has no such setting. The pane shows a red warning line while it is on: "Files in the watched folder are moved to the Trash once they are in the library."
- With it on, a file is removed only after its import committed: the item exists in the library and its original file is in place inside the library. Nothing is removed when the import failed, when the file was held for the duplicate dialog (the dialog still needs it, so it stays in the folder and is remembered), when the file is not a regular file directly inside the watched folder (`<chosen>/<library name>`, compared after resolving links; a link is never followed), or when it has changed (size or modification time) since it was handed to the import. A file moved to the Trash is forgotten from the remembered list, so the same file dropped in again later imports again, through the duplicate check.
- falcon moves the file to the Trash (`NSFileManager trashItemAtURL:resultingItemURL:error:`) instead of unlinking it, so a mistake can be undone from the Trash. Eagle deletes the file. If moving it to the Trash fails, the file stays and is remembered (so it isn't imported again in a loop); the failure is written to the error log, not shown as an import failure.
- With the preference off, files stay in place and falcon remembers each file it handed over (path, size and mtime, the latest 5,000, saved with the app state) and doesn't import it again while it stays unchanged, across launches too. A file changed in place, or replaced with a different one, is imported again.
- The decision logic (`auto_import::may_remove`, `after_import` and `Removal::finish`) is unit tested on temp folders with the deletion injected: a committed import removes the file; a failed or held import, deleting off, a file outside the watched folder or changed since, and a Trash failure all keep it.
- falcon has no file-event crate, so a background thread lists the folder every two seconds. A file is imported once its size and mtime have stayed the same for three seconds (`Settle`).
- The skip rules are Eagle's (`auto_import::skip`): `.tmp` anywhere in the name, folders, no extension, `.url`, 1,000 bytes or smaller (SVGs: empty). falcon also skips hidden files (`.DS_Store`, `._` files) and unfinished browser downloads, as its drops do, and imports any other format, as a drop does, where Eagle takes only its supported formats.
- The files of each pass go through the usual background import into the library's root with no folders or tags, so the duplicate check, IPTC keywords, font style tags and the failed-imports list apply. Names are cleaned as for any import (forbidden characters); `%` and emoji are kept.
- With notices on and "Say when files from the auto-import folder start importing" on (both default), each pass shows "Importing N files from the auto-import folder".
