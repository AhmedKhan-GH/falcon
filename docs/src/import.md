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

**Audio (observed).** mp3, wav, flac, ogg, aac and m4a. `width` 280 and `height` 140 (the waveform thumbnail, see [Thumbnails](thumbnails.md)), then `duration` from an `<audio>` element. `bpm` comes from a tempo detector when one is found. Falcon doesn't detect tempo, so it never writes `bpm`.

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
- **`fontMetas`.**
  - Read with `ttf-parser` (ttf, otf, the first face of ttc or otc) after falcon's own WOFF 1 and WOFF 2 unpackers (`fontmeta::sfnt`).
  - WOFF 2 goes further than Eagle: falcon writes the same opentype.js-shaped `fontMetas` (and weight tag) as for the plain font. The Brotli stream is decoded with `brotli-decompressor`, and the tables are rebuilt into an sfnt. Tables stored with a WOFF 2 transform (the packed `glyf` and `loca`, and a transformed `hmtx`) are left out, since names, `maxp` and `cmap` are all metadata needs. WOFF 2 collections are not read.
  - A TrueType woff2 with packed glyphs therefore has no outlines for the 600×600 specimen. Its thumbnail falls back to QuickLook. CFF-flavoured woff2 and woff2 files with plain `glyf` get the specimen.
  - Language codes cover the common Windows and Mac Roman languages. Records in other languages or Mac encodings are skipped, where opentype.js would decode them.
  - Collections get the opentype.js shape, not Typr's.
  - Eagle's MD5 fallback for fonts with a family name but no English PostScript name is not reproduced.
  - `ImportOptions::font_weight_tag` (default true, as in Eagle) adds the weight tag.

### Duplicates in falcon

The `duplicates` module compares file contents instead of copying Eagle's metadata key, and writes nothing to the library.

- **Exact copies.** `find_exact_duplicates` groups items, trash excluded, whose originals hash to the same BLAKE3 digest. Only files that share their size with another file are hashed.
- **Near copies.** `find_near_duplicates(lib, threshold)` takes a 64-bit perceptual hash of each item's thumbnail, or of the original for small images shown without one. The hash is a 32×32 grey DCT, keeping the 8×8 lowest frequencies, each compared with their median. Items within `threshold` bits are grouped, transitively. About 10 suits resized or recompressed copies.
- **Cache.** The `_cached` variants take a `HashCache`, keyed by item id and invalidated when the original's size or mtime changes. `HashCache::save` and `load` keep it as JSON wherever the caller likes.
- **Results.** Groups list item ids in library order.

### Keywords in falcon

`ImportOptions::keywords_as_tags` is off by default, as in Eagle. When it is on, the `keywords` module reads:

- jpg and jpeg: the IPTC block in APP13, parsed as proper IIM records (record 2, dataset 25), and the XMP packet in APP1.
- png: the XMP packet in the `iTXt` chunk `XML:com.adobe.xmp`, compressed or not.
- tif and tiff: the IPTC (tag 33723) and XMP (tag 700) entries of the first IFD.

IPTC keywords come first, then the XMP `dc:subject` items. Eagle's 200-character limit and `;` split apply to both. Falcon also drops empty parts, where Eagle would keep an empty tag. Reading `.jpeg`, XMP, PNG and TIFF goes beyond what Eagle does.
