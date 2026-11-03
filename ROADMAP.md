# Roadmap

Falcon is early alpha. This page summarises what is done, what is left and what comes later. The detail lives in three files, which are the source of truth:

- [PARITY.md](PARITY.md): every Eagle 4.0.0 feature, with falcon's status checked against its code.
- [FORMATS.md](FORMATS.md): every file format falcon handles, compared with Eagle's set.
- [PLUGINS.md](PLUGINS.md): everything Eagle provides through plugins, and what falcon would build.

## Done

Of the 909 local features in PARITY.md, 883 are done, 17 are partly done and 9 are missing. That is about nineteen in twenty. A further 43 need an online service, an account or Eagle's plugin system and are counted separately.

- **Library:** reads and writes Eagle-format libraries, byte-identical on round trips, with backups, restore, duplicate detection, `.eaglepack` import and export, a watched import folder, reloading on outside changes and sync conflict detection.
- **Import:** thumbnails and colour palettes, folders with their structure, links, paste, drag and drop, screen capture and background imports.
- **Organise:** folders, smart folders, tags and tag groups, Quick Access, ratings, notes, comments, trash, batch editing, undo and a tag picker.
- **Search:** keyword syntax, typo tolerance, the filter bar, saved filters, colour similarity search and sorting.
- **Browse:** four layouts, marquee selection, hover previews that play the file itself, themes and a native menu bar.
- **Viewers:** images at full resolution, GIF and animated WebP and AVIF, video with an FFmpeg fallback that includes AV1 and keeps the pitch at other speeds, audio, fonts, a text editor, the PDF reader with its tools and its layer tree, interactive 3D with textures, full IFC through web-ifc, PSD and AI at full resolution, and live web pages and MHTML.
- **File formats:** exactly Eagle's set, listed in [FORMATS.md](FORMATS.md).
- **App:** preferences panes, localisation into eight languages, sound effects, Actions and Export in Format built into the app, the local HTTP API and the command-line tool.

## Left

Local features still missing or partly done in [PARITY.md](PARITY.md), by theme. Most of the partly done rows name a small gap, or wait for a person to check them on screen.

- **File ▸ New… templates:** Sketch, Photoshop, Illustrator, text, Word, Excel, PowerPoint and XMind documents are written by falcon. Keynote, Pages, Numbers, XD and MindNode are listed but disabled: closed formats, deferred by the user's decision ([FORMATS.md](FORMATS.md) says why for each).
- **3D:** the normal, bump and emissive maps and alpha blending are drawn but not yet compared by eye with Eagle. Some IFC readers also have gaps, which the row names.
- **Accepted limits:** PPT, PPTX, POTX and PostScript-only AI files stay capped at 8192 px, and the preview window at 3072 px (macOS hands PowerPoint files back only as a bitmap or as HTML, with no PDF for PDFKit); an EPS with no preview picture gets no thumbnail (no PostScript interpreter); no per-image colour management behind the Color Space menu.
- **Vibrancy:** with the preference on (off by default), menus and boxes are translucent and only the Preferences and Collect windows get a real blur of the desktop; Eagle blurs behind its in-window menus too.
- **Differences by design or by platform:** falcon's own font folder, tasks that keep running when you switch libraries (with a warning first), and decoders that only exist on macOS, which is the only platform.
- **Waiting for a check by a person:** hover video and its sound, the switch warning, the screenshot notice with a real pointer, the grayed web and 3D panes, the PDF layer tree on screen and the blank documents in their own apps.
- **Not feasible today:** a GPU acceleration switch.
- **Translation:** the eight languages are machine written and wait for a native read.

## Later

- **Plugins:** Combine Images, Search by Image, the Eagle MCP server, AI Models and AI Search, in the order given in [PLUGINS.md](PLUGINS.md). Falcon has no plugin system and none is planned; each would be a built-in feature.
- **Out of scope:** Windows and Linux builds (falcon runs on macOS only, and several viewers use macOS frameworks), a GPU acceleration switch, Eagle's script and inject features, and every online feature.
- **Formats beyond Eagle's set:** listed at the end of [FORMATS.md](FORMATS.md) and not planned.
- **Releases:** signed and notarised macOS builds. Falcon is unsigned today.
