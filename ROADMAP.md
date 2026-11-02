# Roadmap

Falcon is early alpha. This page summarises what is done, what is left and what comes later. The detail lives in three files, which are the source of truth:

- [PARITY.md](PARITY.md): every Eagle 4.0.0 feature, with falcon's status checked against its code.
- [FORMATS.md](FORMATS.md): every file format falcon handles, compared with Eagle's set.
- [PLUGINS.md](PLUGINS.md): everything Eagle provides through plugins, and what falcon would build.

## Done

Of the 901 local features in PARITY.md, 854 are done, 29 are partly done and 18 are missing. That is about nineteen in twenty. A further 42 need an online service, an account or Eagle's plugin system and are counted separately.

- **Library:** reads and writes Eagle-format libraries, byte-identical on round trips, with backups, restore, duplicate detection, `.eaglepack` import and export, a watched import folder, reloading on outside changes and sync conflict detection.
- **Import:** thumbnails and colour palettes, folders with their structure, links, paste, drag and drop, screen capture and background imports.
- **Organise:** folders, smart folders, tags and tag groups, Quick Access, ratings, notes, comments, trash, batch editing, undo and a tag picker.
- **Search:** keyword syntax, typo tolerance, the filter bar, saved filters, colour similarity search and sorting.
- **Browse:** four layouts, marquee selection, hover preview, themes and a native menu bar.
- **Viewers:** images at full resolution, GIF and animated WebP and AVIF, video with an FFmpeg fallback that includes AV1, audio, fonts, a text editor, the PDF reader with its tools, interactive 3D with textures, full IFC through web-ifc, PSD and AI at full resolution, and live web pages and MHTML.
- **File formats:** exactly Eagle's set, listed in [FORMATS.md](FORMATS.md).
- **App:** preferences panes, localisation into eight languages, sound effects, Actions and Export in Format built into the app, the local HTTP API and the command-line tool.

## Left

Local features still missing or partly done in [PARITY.md](PARITY.md), by theme.

- **Shortcuts and menus:** the keys T, F, ⌘J, ⇧-arrows, ⌘T, F5 and ⌘O, and gaps in the item and folder context menus.
- **Viewer:** presentation mode, a separate preview window, a viewer context menu, zoom presets, a navigator, video frame tools and per-item viewer backgrounds.
- **Search and organising:** size and dimension filter chips, a comment filter and search scope, Filter Lock, ratio shapes, batch rename of items, merging duplicates and rating shortcuts.
- **Library:** the `eagle://` scheme, merging another library, library list management, and safety checks for risky paths.
- **Preferences:** the settings and panes not yet built, listed in section 6 of the import, export and library part of [PARITY.md](PARITY.md).
- **Translation:** the eight languages are machine written and wait for a native read.

PARITY.md is the authority on the exact rows; some of its ranked lists still carry entries marked fixed.

## Later

- **Plugins:** Combine Images, Search by Image, the Eagle MCP server, AI Models and AI Search, in the order given in [PLUGINS.md](PLUGINS.md). Falcon has no plugin system; each would be a built-in feature.
- **Platforms:** Windows and Linux builds. Falcon runs on macOS only, and several viewers use macOS frameworks.
- **Releases:** signed and notarised macOS builds. Falcon is unsigned today.
- **Formats beyond Eagle's set:** listed in [FORMATS.md](FORMATS.md) and not planned yet.
