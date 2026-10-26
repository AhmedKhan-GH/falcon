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

## How falcon does it

The `thumbnail` module holds the rules as plain functions: `plan_raster`, `plan_svg`, `target_size`, `max_side`, `resize` and `encode`.

- **Rules.** Falcon applies the flag, size and encoding rules above exactly, including the short-side quirk and the PNG exception.
- **Formats.** png, jpg/jpeg, webp, gif (first frame), bmp and svg. Other extensions have no thumbnail. The 480 and 512 limits are in the table, but no format that uses them can be decoded yet. The list of 480 extensions is partial.
- **Decoding.** The `image` crate. EXIF orientation is applied, so `width` and `height` are the displayed size, as with Eagle's `<img>` measurement.
- **Resizing.** One Lanczos3 resize straight to the target, with no halving steps. Pixels differ slightly from Eagle's.
- **Encoding.** The `webp` crate (libwebp) at quality 75, always with an alpha channel. Past 16383 px, `image`'s JPEG encoder at quality 70.
- **SVG.** Rendered with `resvg`, directly at the target size. System fonts are not loaded, so text in an SVG does not render. A small SVG is rendered at its natural size, because its thumbnail only feeds the palette. Width and height are the SVG's own size, rounded up.
- **Reading back.** `thumbnail::decode` sniffs the content, so it reads a thumbnail even though its name ends in `.png`.
