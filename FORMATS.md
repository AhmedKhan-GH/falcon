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
| arw, cr2, cr3, crw, dng, raf, rw2, orf, nef, nrw, raw, 3fr, erf, srw, sr2, pef, x3f, mrw | Embedded full-size preview | Embedded full-size preview, correctly oriented | Done |

## Design and creative files

| Formats | Eagle | Falcon | Status |
| --- | --- | --- | --- |
| psd, psdt, psb | High-resolution render | The file's merged image decoded by falcon at full resolution, in colour through its ICC profile; QuickLook when it has none (Maximize Compatibility off) | Done |
| ai, ait | High-resolution render | PDF-compatible files: PDFKit draws the part on screen at the size shown, sharp at any zoom in the viewer. PostScript-only files: QuickLook, redrawn at each zoom level up to 8192 px | Done |
| eps | Image of the thumbnail (dropped on macOS 13+) | QuickLook render (macOS 14+ has no EPS renderer) | Done |
| xd, sketch, fig, af, afdesign, afphoto, afpub, c4d, blend, clip, skp, dwg, prd, cdr, skt, pxd, graffle, mindnode, xmind, eddx, emmx, indd, indt, idml | Image of the thumbnail | QuickLook render where the app's generator is installed | Done |

## Documents

| Formats | Eagle | Falcon | Status |
| --- | --- | --- | --- |
| pdf | pdf.js reader inside the window | PDFKit reader inside the window: find, rotate, spreads, links, password, attachments, switchable layers, print | Partial: the layers list is flat rather than the document's tree, and printing ignores switched layers |
| txt | Editable text | Editable text with autosave | Done |
| doc, docx, xls, xlsx, key, pages, numbers | Image of the thumbnail | QuickLook render | Done |
| ppt, pptx, potx | High-resolution render | QuickLook, redrawn at each zoom level up to 8192 px | Done |

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
| glb, obj, fbx, 3ds, 3mf, dae, ply, stl | Interactive viewer with textures | Interactive viewer and thumbnails with diffuse textures for glb, obj, fbx, dae and 3ds (3mf, ply and stl have no textures in Eagle's viewer either); export of geometry and colours | Done |
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

## Possible later additions (not planned)

These are open formats in common use that neither app handles well:

- epub
- md
- csv, json and other plain text
- odt, ods, odp
- usdz
- jp2
