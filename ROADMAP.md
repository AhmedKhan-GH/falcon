# Roadmap

Falcon is early alpha. This page summarises what is done, what is left and what comes later. The detail lives in three files, which are the source of truth:

- [PARITY.md](PARITY.md): every Eagle 4.0.0 feature, with falcon's status checked against its code.
- [FORMATS.md](FORMATS.md): every file format falcon handles, compared with Eagle's set.
- [PLUGINS.md](PLUGINS.md): everything Eagle provides through plugins, and what falcon would build.

## Done

Of the 912 features in PARITY.md outside the online set, 905 are done and 7 are accepted differences; none is partly done or missing. That is about 99% done. A further 32 need an online service, an account or Eagle's plugin system and are counted separately.

- **Library:** reads and writes Eagle-format libraries, byte-identical on round trips, with backups, restore, duplicate detection, `.eaglepack` import and export, a watched import folder, reloading on outside changes and sync conflict detection.
- **Import:** thumbnails and colour palettes, folders with their structure, links, paste, drag and drop, screen capture and background imports.
- **Organise:** folders, smart folders, tags and tag groups, Quick Access, ratings, notes, comments, trash, batch editing, undo and a tag picker.
- **Search:** keyword syntax, typo tolerance, the filter bar, saved filters, colour similarity search and sorting.
- **Browse:** four layouts, marquee selection, hover previews that play the file itself, themes and a native menu bar.
- **Viewers:** images at full resolution, GIF and animated WebP and AVIF, video with an FFmpeg fallback that includes AV1 and keeps the pitch at other speeds, audio, fonts, a text editor, the PDF reader with its tools and its layer tree, interactive 3D with textures, full IFC through web-ifc, PSD and AI at full resolution, and live web pages and MHTML.
- **File formats:** exactly Eagle's set, listed in [FORMATS.md](FORMATS.md).
- **AI:** on-device AI Search, AI Models with Eagle's eight providers (tested for real on Ollama only), AI steps in Actions and a local MCP server, none of them yet seen on screen.
- **App:** preferences panes, localisation into eight languages, sound effects, Actions and Export in Format built into the app, the local HTTP API and the command-line tool.

## Left

No local feature in [PARITY.md](PARITY.md) is missing or partly done. What follows is the accepted differences from Eagle, by theme, and the checks and work still open.

- **File ▸ New… templates:** Sketch, Photoshop, Illustrator, text, Word, Excel, PowerPoint and XMind documents are written by falcon. Keynote, Pages, Numbers, XD and MindNode are listed but disabled: closed formats, deferred by the user's decision ([FORMATS.md](FORMATS.md) says why for each).
- **3D:** the normal, bump and emissive maps and alpha blending are drawn and pass tests, but weren't compared with Eagle, by the user's decision. A few rare IFC shapes are skipped, as in web-ifc itself.
- **Accepted limits:** PPT, PPTX, POTX and PostScript-only AI files stay capped at 8192 px, and the preview window at 3072 px (macOS hands PowerPoint files back only as a bitmap or as HTML, with no PDF for PDFKit); an EPS with no preview picture gets no thumbnail (no PostScript interpreter); colours outside sRGB are clipped, since pictures with a colour profile are converted to sRGB for showing; no blur behind menus and boxes inside a window.
- **Vibrancy:** with the preference on (off by default), menus and boxes are translucent and only the Preferences and Collect windows get a real blur of the desktop; Eagle blurs behind its in-window menus too.
- **Differences by design or by platform:** falcon's own font folder, tasks that keep running when you switch libraries (with a warning first), and decoders that only exist on macOS, so Linux goes without them for now.
- **Not yet checked on screen:** the switch warning, the screenshot notice with a real pointer, the PDF layer tree and print panel, right-click Remove in the folder picker, and ⇧⌘J. Hover video's picture and the grayed web and 3D panes have been seen working; its sound and the blank documents in their own apps haven't been checked and aren't planned to be.
- **Left out by decision:** a GPU acceleration switch, as falcon has only one graphics backend.
- **Translation:** the eight languages are machine written and wait for a native read.

## Later

- **Plugins:** Search by Image isn't planned, and there's no plugin system. The Eagle MCP server, AI Models and AI Search are built in ([PLUGINS.md](PLUGINS.md)). Falcon has no plugin system and none is planned; each would be a built-in feature.
- **Out of scope:** Windows builds, a GPU acceleration switch, Eagle's script and inject features, and every online feature.
- **Formats beyond Eagle's set:** listed at the end of [FORMATS.md](FORMATS.md) and not planned.
- **Linux:** the macOS-only parts still to bring over: a PDF engine and reader, web views, screen capture, HEIC, AVIF animation and JPEG XL decoding, QuickLook's thumbnails, Open With's app list, dragging out of the window, the tray icon and notifications. macOS stays the primary platform; Linux must always build and pass its tests.
- **Releases:** signed and notarised macOS builds, and downloadable macOS builds at all: only Linux has them. Falcon is unsigned today.
