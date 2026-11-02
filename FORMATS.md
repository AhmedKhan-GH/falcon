# File formats

This is the full list of file formats falcon handles, compared with Eagle 4.0.0 on macOS. Eagle's list comes from its `SUPPORT_FORMATS_DARWIN` setting. Any file outside these lists still imports as a plain item, with a QuickLook thumbnail where macOS has one, as in Eagle.

The goal for now is Eagle's set. Formats beyond it are listed at the end, but they aren't planned yet.

Status: **Done** works as well as Eagle or better. **Partial** opens, but something Eagle does is missing; the reason is given.

Eagle's alias extensions import as the format they stand for, whatever macOS calls them: jfif, jpe and insp are JPEG, psdt is psd, hif is heic, tif is tiff, ait is ai, indt is indd and m2ts is mts. falcon reads JPEGs by their contents, and hands QuickLook a file under the main extension where macOS has no type for the alias (jfif, psdt).

## Images

| Formats | Eagle | Falcon | Status |
| --- | --- | --- | --- |
| jpg, jpeg, jfif, jpe, insp, png, bmp | Image viewer | Full resolution, tiled for large images | Done |
| gif | Viewer with frame player | All frames, Eagle's frame player, plays on hover | Done |
| webp | Image viewer, animates | Full resolution, animates, plays on hover | Done |
| avif | Image viewer, animates | Full resolution and animation through macOS ImageIO | Done |
| jxl | Image viewer | Full resolution through ImageIO (macOS 14+) | Done |
| heic, heif, hif | Image viewer | Full resolution and orientation through ImageIO | Done |
| tif, tiff | TIFF viewer | Full resolution, multi-page paging | Done |
| tga | TGA viewer | Full resolution | Done |
| hdr, exr | Image of the thumbnail | Tone-mapped viewer with an exposure slider | Done |
| ico, icns, dds | Image of the thumbnail | QuickLook render | Done |
| svg | Vector viewer | Redrawn sharp at every zoom | Done |

## RAW photos

| Formats | Eagle | Falcon | Status |
| --- | --- | --- | --- |
| arw, cr2, cr3, crw, dng, raf, rw2, orf, nef, nrw, raw, 3fr, erf, srw, sr2, pef, x3f, mrw | Embedded full-size preview | Embedded full-size preview, correctly oriented; also the thumbnail and the camera's size at import when QuickLook has no render | Done |

## Design and creative files

| Formats | Eagle | Falcon | Status |
| --- | --- | --- | --- |
| psd, psdt, psb | High-resolution render | The file's merged image decoded by falcon at full resolution, in colour through its ICC profile; QuickLook when it has none (Maximize Compatibility off) | Done |
| ai, ait | High-resolution render | PDF-compatible files: PDFKit draws the part on screen at the size shown, sharp at any zoom in the viewer. PostScript-only files: QuickLook, redrawn at each zoom level up to 8192 px. With no QuickLook render: the thumbnail is page 1 through PDFKit, or the preview in a PostScript file (see eps), which stands in for the viewer | Done |
| eps | Image of the thumbnail (dropped on macOS 13+) | QuickLook render (macOS 14+ has no EPS renderer), else the preview in the file: a DOS header's TIFF, a `%%BeginPreview` bitmap or an `%AI7_Thumbnail` block. An EPS with none gets no thumbnail (falcon has no PostScript interpreter) | Partial |
| xd, sketch, fig, af, afdesign, afphoto, afpub, c4d, blend, clip, skp, dwg, prd, cdr, skt, pxd, graffle, mindnode, xmind, eddx, emmx, indd, indt, idml | Image of the thumbnail | QuickLook render where the app's generator is installed; sketch, xmind, indd, indt, idml, blend, skp and dwg fall back to the preview picture in the file, where it has one (not a zstd-compressed blend) | Done |

## Documents

| Formats | Eagle | Falcon | Status |
| --- | --- | --- | --- |
| pdf | pdf.js reader inside the window | PDFKit reader inside the window: find, rotate, spreads, links, password, attachments, switchable layers, print | Partial: the layers list is flat rather than the document's tree, and printing ignores switched layers |
| txt | Editable text | Editable text with autosave | Done |
| doc, docx, xls, xlsx, key, pages, numbers | Image of the thumbnail | QuickLook render; key falls back to the preview picture in the file, where it has one (many have none) | Done |
| ppt, pptx, potx | High-resolution render | QuickLook, redrawn at each zoom level up to 8192 px | Partial: past 8192 px the picture stops getting sharper. macOS hands these files back only as a bitmap or as HTML, with no PDF to draw by region |

## Fonts

| Formats | Eagle | Falcon | Status |
| --- | --- | --- | --- |
| ttf, otf, ttc, woff, woff2 | Font viewer | Font viewer with tabs, themes, info, rename and activation | Done |

## Audio

| Formats | Eagle | Falcon | Status |
| --- | --- | --- | --- |
| mp3, wav, ogg, flac, aac, m4a | Player with waveform | Player with waveform, falcon's own decoder (Opus in ogg; AAC and ALAC in m4a) | Done |

## Video

| Formats | Eagle | Falcon | Status |
| --- | --- | --- | --- |
| mp4, mov, m4v, 3gp | HTML5 player | AVFoundation player; FFmpeg (AV1 through dav1d) when AVFoundation can't decode a file | Done |
| mkv, webm, avi, wmv, flv, f4v, mpg, ts, mts, m2ts, 360, afx, eva, vap | HTML5 player or mpv | FFmpeg player (LGPL, decode only, AV1 through dav1d), thumbnail and duration at import | Done |

## 3D models

| Formats | Eagle | Falcon | Status |
| --- | --- | --- | --- |
| glb, obj, fbx, 3ds, 3mf, dae, ply, stl | Interactive viewer with textures, normal, bump and emissive maps and transparency | Interactive viewer and thumbnails with diffuse textures for glb, obj, fbx, dae and 3ds (3mf, ply and stl have no textures in Eagle's viewer either); the maps and transparency Eagle draws: normal maps for glb, fbx and dae, bump maps for obj and fbx, emissive for glb, transparency for glb, obj, fbx, dae and 3ds; export of geometry and colours | Partial: the maps and transparency pass tests on a real GPU but haven't yet been checked by eye against Eagle |
| ifc | Interactive viewer with full geometry (web-ifc) | Interactive viewer and thumbnails with full geometry through web-ifc 0.0.78 (swept, CSG, boolean and b-spline solids, openings cut), built from source by `scripts/fetch-web-ifc.sh` and run in a worker process | Done |

## Web

| Formats | Eagle | Falcon | Status |
| --- | --- | --- | --- |
| url, html | Live page | Live page in the viewer (WKWebView) | Done |
| mhtml | Live page | Live page in the viewer and preview window (WKWebView), unpacked by falcon's own MIME reader with its styles and images and kept off the network; thumbnail from the unpacked page | Done |

## Libraries and packages

| Formats | Eagle | Falcon | Status |
| --- | --- | --- | --- |
| .library | Opens | Opens (only inside the sandbox folder during development) | Done |
| .eaglepack | Imports | Imports and exports | Done |

## Already in falcon beyond Eagle's set

None.

## How well each format has been tested

Falcon is tested against a local library holding one sample of each format where a sample is available. It covers 101 of Eagle's 124 extensions. The library isn't published, because some samples are for local testing only.

**No sample (23 extensions).** These come from closed or paid apps, and a limited search of public test-file collections found no free sample. The code paths exist but haven't been run on a real file:

- **Images:** insp
- **Design:** ait, xd, fig, af, afdesign, afphoto, afpub, c4d, clip, prd, cdr, skt, pxd, graffle, mindnode, eddx, emmx, indt
- **Video:** 360, afx, eva, vap

**Samples that only show a format opens.** These are small or generated, so complex real-world files could behave differently:

| Formats | Sample |
| --- | --- |
| ai | A header-only stub in the test library; real files are still to be tried |
| doc, docx | Generated with macOS `textutil` |
| xlsx, pptx | Generated with a script |
| xls, ppt | Apache POI's test files |
| 3D models | Mostly small cubes, plus one textured sample and one larger FBX |
| Most video | Short generated clips in hand-written containers |
| jpeg, jfif, jpe, heif, hif, tif, psdt | Copies of another sample under an alternate extension |

**Not yet confirmed:**

- Blank documents from File ▸ New… haven't been opened in Word, Excel, PowerPoint or XMind.
- A sample passing means it opened in falcon, not that it looks identical to Eagle.

## Possible later additions (not planned)

These are open formats in common use that neither app handles well:

- epub
- md
- csv, json and other plain text
- odt, ods, odp
- usdz
- jp2
