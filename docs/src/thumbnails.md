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
