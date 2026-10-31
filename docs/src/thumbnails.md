# Thumbnails

## What Eagle does

Every claim is from the renderer code in `app/js/thumbs/` and `app/js/utils/compress.js`.

### Whether an item gets a thumbnail

- For png, jpg, bmp and jxl, `noThumbnail` is true when width and height are at most 960 and the file is at most 15,000,000 bytes (observed). The original is then displayed directly and no thumbnail file is kept.
- A png of at most 960 px on both sides but over 1 MiB still gets a thumbnail (observed).
- webp and animated avif have their own checks (observed that they exist; not read). Falcon assumes the same rule for webp and gif (inferred).
- SVG always renders a WebP, which is used for the palette. Under 100,000 bytes, `removeThumbnail` and `noThumbnail` are both set and the file is deleted after the palette is computed. At 100,000 bytes or more the thumbnail stays and neither flag is written. A render error sets `noThumbnail` only (observed).
- The flags are absent on items that simply have a thumbnail (observed in the fixture).

### Size

- The limit is 320 for ordinary images, 480 for documents, design files and 3D formats, and 512 for ico and icns (observed).
- Nothing is resized unless the longer side is over the limit (observed).
- Then the short side becomes the limit. For landscape (`w > h`) the height is the limit and the width is `floor(limit * w / h)`. Otherwise the width is the limit and the height is `floor(limit * h / w)` (observed). So 2000x1000 becomes 640x320, and 1000x200 becomes 1600x320, which is an upscale of the short side. This looks like an accident, not intent (inferred).
- Large sources are halved on a canvas until within 1.5x of the target, with high-quality smoothing, then drawn at the target size (observed).

### Encoding

- Lossy WebP at quality 0.75, written to `<name>_thumbnail.png`. The bytes are WebP despite the name (observed).
- If a side exceeds 16383 px, JPEG at 0.7 is written under the same name (observed).
- All of this is done by Chromium's image decoder and canvas (observed). The exact output bytes therefore cannot be reproduced.

### Audio waveforms (observed)

- An audio item's thumbnail is its waveform.
- Eagle draws it with wavesurfer.js in an off-screen 280×140 container. It uses colour #0072EF, normalised to the loudest peak, on a transparent background.
- The canvas is saved as PNG and then compressed to WebP like any other thumbnail. Sides are multiplied by the display's pixel ratio, so Retina renders are probably larger (inferred).
- The item's `width` and `height` are 280 and 140.
- Files longer than an hour only have their first 10 MB loaded.
- No peak data is cached on disk (no `waveform` or `peaks` file in the item folder). The player re-decodes the file when it shows one.

### Font specimens (observed)

A font's thumbnail is a 600×600 specimen. The item's `width` and `height` are 600:

- The background is white and the text is #333.
- The font is loaded as a web font, and html2canvas captures the block as JPEG at quality 1. It is then compressed to a 320×320 WebP.
- For Latin fonts there are three lines, `ABCDEFG`, `abcdefg` and `0123456`, at 90 px with line height 1.35. They sit left-aligned in a 400 px block centred on the canvas.
- Fonts that support Chinese, Japanese or Korean get two lines of sample characters at 100 px (one line at 90 px for Korean), then `AaBbCcDd` and `01234567` at 82 px, in a 380 px block.
- The script is chosen from `fontMetas.support` and the languages of `postScriptName`.
- Missing glyphs fall back to an outline font.

### Custom thumbnails (observed)

From the context menu's "Custom Thumbnail" entries and the background window's `set-custom-thumbnail` handler:

- The command is hidden for tif, jpg, png, bmp and webp items, whose file is already their picture. Every other format can have one.
- The file dialog offers jpg, jpeg, png, bmp and webp. A file over 10,000,000 bytes is refused with a warning. "From clipboard" also takes a copied gif file, or saves a copied image as PNG first.
- The chosen file is copied **byte for byte** to `<name>_thumbnail.png`. It is not resized or re-encoded, so a custom thumbnail can be a large JPEG under a `.png` name.
- The item's `width` and `height` become the picture's size, even for a video or PDF (the HTTP API may pass its own `width` and `height`).
- The palette is computed from the new file.
- `noThumbnail` and `noPreview` are deleted, and `customThumbnail: true` is set. Being a new key, it lands at the end of the item. `removeThumbnail` is left alone.
- Grid cards with a custom thumbnail get a `custom-thumbnail` CSS class. The viewer and the plugin API use the thumbnail in place of the original for formats Eagle can't show itself.
- Refresh Thumbnail skips an item whose `customThumbnail` is set. "Reset Thumbnail" (shown only when it is set) deletes the key and then refreshes the thumbnail.

### Refresh Thumbnail (observed)

The `regenerate-thumbnail` queue reads the original's size, birth time and mtime again (`size`, `btime`, `mtime`), then runs the same per-format generator as import, so `width`, `height` and the flags are measured again. The palette is then queued the same way as at import. A format Eagle no longer recognises loses its thumbnail, `palettes`, `width` and `height` and gets `noPreview: true`. When a re-measured image becomes small enough for `noThumbnail`, the jpg generator sets the flag but does not delete the old thumbnail file (inferred harmless, since Eagle then shows the original).

## Crop and rotate (observed)

From the viewer's `rotateImage`, `cropImage` and `saveCrop`, `js/utils/rotateImage.js` and `my_modules/image-cropper`:

- **Where.** Both are buttons in the viewer's toolbar. Rotate (⇧R) turns 90° anticlockwise, or clockwise with ⇧ held while clicking. It is offered for jpg, png, webp and bmp. Crop is offered for jpg, png and webp, and is disabled for animated images.
- **Rotation is written to the file** when the `imageRotateMode` preference is `write`, which is the default. The other mode only turns the view.
- **JPEG rotation is lossless.** Only the EXIF Orientation tag changes, following a table of the eight orientations. The compressed image is untouched.
- **Other formats are re-encoded.** PNG, WebP and BMP are drawn rotated on a canvas and exported in the same format, at quality 1 for WebP. Animated PNG is refused, and AVIF can't be written.
- **Cropping is always re-encoded.** The crop box is cut out on a canvas and exported as PNG, JPEG or WebP, with a quality setting of 100 passed to Eagle's canvas helper (inferred to mean the best quality). The canvas drops the EXIF block, so a JPEG's orientation ends up in its pixels. Eagle shows the result and asks before saving.
- **Writing.** Crop and rotate both overwrite the original in place and keep its name. Crop first copies it to `<file>.bk` and restores that if the write fails. Rotate writes `<file>.tmp` and then the file.
- **Afterwards.** Eagle refreshes the thumbnail (see above), which updates `size`, `btime`, `mtime`, `width`, `height`, the thumbnail and the palette. The item edit also bumps `lastModified`; `modificationTime` stays. Folder covers that show the item are redrawn.
- **Undo.** There is none. The overwritten pixels are gone.

## How falcon does it

The `thumbnail` module holds the rules as plain functions: `plan_raster`, `plan_svg`, `target_size`, `max_side`, `resize` and `encode`.

- **Rules.** Falcon applies the flag, size and encoding rules above exactly, including the short-side quirk and the PNG exception.
- **Formats.** png, jpg/jpeg, webp, gif (first frame), bmp and svg. Other extensions have no thumbnail. The 480 and 512 limits are in the table, but no format that uses them can be decoded yet. The list of 480 extensions is partial.
- **Decoding.** The `image` crate. EXIF orientation is applied, so `width` and `height` are the displayed size, as with Eagle's `<img>` measurement.
- **Resizing.** One Lanczos3 resize straight to the target, with no halving steps. Pixels differ slightly from Eagle's.
- **Encoding.** The `webp` crate (libwebp) at quality 75, always with an alpha channel. Past 16383 px, `image`'s JPEG encoder at quality 70.
- **SVG.** Rendered with `resvg`, directly at the target size. System fonts are not loaded, so text in an SVG does not render. A small SVG is rendered at its natural size, because its thumbnail only feeds the palette. Width and height are the SVG's own size, rounded up.
- **Reading back.** `thumbnail::decode` sniffs the content, so it reads a thumbnail even though its name ends in `.png`.
- **QuickLook.** Formats falcon can't decode are rendered with `qlmanage`. A render that takes more than 30 seconds is abandoned, since qlmanage can hang on malformed files.
- **Audio.**
  - `waveform::peaks` decodes the file with symphonia and returns (min, max) pairs of the mono mix for any number of buckets.
  - `waveform::render` draws them one column per bucket, normalised and mirrored around the middle.
  - Import draws 280 buckets at 280×140 in #0072EF on transparency, sets `width` and `height` to 280 and 140, and stores that as the thumbnail. There is no pixel-ratio scaling.
  - Nothing is cached, as in Eagle. If decoding fails, QuickLook is tried instead.
- **Fonts.**
  - `fontpreview::render` loads only the font into a `fontdb`, lays out the specimen as SVG text and draws it with resvg at 600×600. Import then scales it to 320×320.
  - Baselines follow CSS: half the leading, then the font's hhea ascent.
  - The layout and sizes are Eagle's. The CJK sample words are falcon's own.
  - Missing glyphs are left blank instead of drawn in an outline font.
  - The result is opaque RGB, as Eagle's JPEG capture is.
  - woff2 is not supported and falls back to QuickLook.
- **Custom thumbnails.** `Library::set_custom_thumbnail(id, path)` follows Eagle: the same format and 10 MB rules, a byte-for-byte copy, `width` and `height` from the picture, a new palette, `noThumbnail` and `noPreview` removed and `customThumbnail: true` appended. It also refuses a file it can't decode. `reset_custom_thumbnail` removes the key and redraws. `thumbnail::has_custom_thumbnail` and `custom_thumbnail_allowed` hold the rules; jpeg and tiff count as jpg and tif.
- **Refresh Thumbnail.** `Library::refresh_thumbnail(id)` re-reads size and file times, then redraws with import's own pipeline and recomputes the palette. It returns false and changes nothing for an item with a custom thumbnail. The new thumbnail replaces the old file only once it has been drawn, so a failed redraw keeps the old one. Unlike Eagle, a thumbnail that is no longer needed is deleted, because falcon shows a thumbnail file whenever one exists.
- **Crop and rotate.** In `transform`: `Library::rotate_item(id, quarter_turns)` (clockwise; negative is anticlockwise) and `Library::crop_item(id, PixelRect)`, in the image's displayed pixels.
  - Formats: rotate takes jpg, png, webp and bmp, crop takes jpg, png and webp. GIF, SVG, PDF and the rest are refused, and so are animated PNG and WebP.
  - A JPEG rotation rewrites only the EXIF orientation. An existing tag is changed in place. EXIF without the tag gets a copy of IFD0 with the tag added, appended to the block. A JPEG without EXIF gets a minimal block after its JFIF header. The compressed image data is copied unchanged.
  - Everything else is decoded with the `image` crate, turned or cut, and encoded again in the same format: PNG, JPEG at quality 100 with no EXIF (as Chromium's canvas does), BMP, and WebP through libwebp. Eagle's canvas exports WebP at quality 1, which falcon takes to be lossy quality 100 (inferred); falcon keeps a lossless WebP lossless.
  - The file is overwritten in place, so its birth time stays. Then the item is refreshed as by Refresh Thumbnail and saved through `update_item`.
  - Undo: falcon's undo can't restore pixels, so these edits mark the open undo transaction irreversible, like a permanent delete. In the app that clears the undo history rather than restoring dimensions that no longer match the file.

