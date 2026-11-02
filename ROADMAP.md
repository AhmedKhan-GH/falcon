# Roadmap

Falcon is early alpha. This page summarises what is done, what is left and what comes later. The detail lives in three files, which are the source of truth:

- [PARITY.md](PARITY.md): every Eagle 4.0.0 feature, with falcon's status checked against its code.
- [FORMATS.md](FORMATS.md): every file format falcon handles, compared with Eagle's set.
- [PLUGINS.md](PLUGINS.md): everything Eagle provides through plugins, and what falcon would build.

## Done

Of the 909 local features in PARITY.md, 866 are done, 34 are partly done and 9 are missing. That is about nineteen in twenty. A further 43 need an online service, an account or Eagle's plugin system and are counted separately.

- **Library:** reads and writes Eagle-format libraries, byte-identical on round trips, with backups, restore, duplicate detection, `.eaglepack` import and export, a watched import folder, reloading on outside changes and sync conflict detection.
- **Import:** thumbnails and colour palettes, folders with their structure, links, paste, drag and drop, screen capture and background imports.
- **Organise:** folders, smart folders, tags and tag groups, Quick Access, ratings, notes, comments, trash, batch editing, undo and a tag picker.
- **Search:** keyword syntax, typo tolerance, the filter bar, saved filters, colour similarity search and sorting.
- **Browse:** four layouts, marquee selection, hover preview, themes and a native menu bar.
- **Viewers:** images at full resolution, GIF and animated WebP and AVIF, video with an FFmpeg fallback that includes AV1, audio, fonts, a text editor, the PDF reader with its tools, interactive 3D with textures, full IFC through web-ifc, PSD and AI at full resolution, and live web pages and MHTML.
- **File formats:** exactly Eagle's set, listed in [FORMATS.md](FORMATS.md).
- **App:** preferences panes, localisation into eight languages, sound effects, Actions and Export in Format built into the app, the local HTTP API and the command-line tool.

## Left

Local features still missing or partly done in [PARITY.md](PARITY.md), by theme. Most of the partly done rows name a small gap, or wait for a person to check them on screen.

- **File ▸ New… templates:** Word, Excel, PowerPoint, XMind and text documents work. Keynote, Pages, Numbers, Sketch, XD, MindNode, PSD and AI are listed but disabled.
- **3D:** normal, bump and emissive maps and alpha blending. Some readers also have gaps, which the row names.
- **Rendering past QuickLook's limit:** PPT, PPTX, POTX and PostScript-only AI files stay capped at 8192 px, and the preview window at 3072 px. macOS hands PowerPoint files back only as a bitmap or as HTML from `Office.qlgenerator`, with no PDF for PDFKit; going through that HTML would need a web view and print path, whose layout would not line up with the thumbnail.
- **PDF layers:** they switch, but appear as a flat list without groups, and printing uses the default layers.
- **Differences by design or by platform:** falcon's own font folder, regex rules without look-around, tasks that continue when you switch libraries, and decoders that only exist on macOS.
- **Waiting for a check on screen:** translucent side panels (off by default) and hover sound.
- **Not feasible today:** a GPU acceleration switch.
- **Translation:** the eight languages are machine written and wait for a native read.

## Later

- **Plugins:** Combine Images, Search by Image, the Eagle MCP server, AI Models and AI Search, in the order given in [PLUGINS.md](PLUGINS.md). Falcon has no plugin system; each would be a built-in feature.
- **Platforms:** Windows and Linux builds. Falcon runs on macOS only, and several viewers use macOS frameworks.
- **Releases:** signed and notarised macOS builds. Falcon is unsigned today.
- **Formats beyond Eagle's set:** listed in [FORMATS.md](FORMATS.md) and not planned yet.
