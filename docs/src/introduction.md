# Introduction

Falcon is a rebuild of the [Eagle](https://eagle.cool) asset manager in Rust, written to learn how Eagle works.

Each chapter in the first part describes one piece of Eagle. It covers what Eagle does, with every claim marked **observed** or **inferred**, and then how falcon implements it.

- **Observed** claims come from Eagle's own files: a real library on disk, or REA analysis of `app.asar`.
- **Inferred** claims are conclusions drawn from those files that haven't been checked directly.

Falcon is written from that behaviour. It doesn't copy Eagle's code.

The second part, [The desktop app](app.md), describes falcon's desktop app: its window, viewers, keys, preferences, themes and how it opens and saves libraries.

## Status

Falcon is early alpha and unsigned. [PARITY.md](https://github.com/AhmedKhan-GH/falcon/blob/main/PARITY.md) compares it with Eagle 4.0.0 feature by feature, checked against falcon's code. Of Eagle's 909 local features, 883 are done, 12 are partly done, 5 are accepted differences and 9 are missing: about nineteen in twenty. Another 43 need an online service, an account or Eagle's plugin system and are counted separately. [ROADMAP.md](https://github.com/AhmedKhan-GH/falcon/blob/main/ROADMAP.md) lists what is left and what comes later.

Falcon handles exactly Eagle's set of file formats. [FORMATS.md](https://github.com/AhmedKhan-GH/falcon/blob/main/FORMATS.md) lists each one and its status. Falcon has no plugin system: Actions and Export in Format are built into the app, and [PLUGINS.md](https://github.com/AhmedKhan-GH/falcon/blob/main/PLUGINS.md) lists the plugins Eagle provides that falcon could build later.

While falcon is in development, the app and the CLI only open libraries inside `~/Falcon Sandbox` (or the folder in `FALCON_SANDBOX`). To work on an existing library, copy it there first. See [Sandbox](app.md#sandbox).

### Working today

- Import with thumbnails and colour palettes, folders with their structure, paste, drag and drop, and screen capture.
- Folders, tags with a tag picker, smart folders, Quick Access, ratings, notes, undo and batch editing.
- Keyword, filter and colour search, with marquee selection in four layouts.
- Viewers for images at full resolution, video (with an FFmpeg fallback that includes AV1), audio, fonts, a text editor, PDFs with their reader tools, interactive 3D models with textures, normal, bump and emissive maps and transparency, full IFC through web-ifc, PSD and AI at full resolution, and live web pages and MHTML.
- Preferences panes, the interface in eight languages (machine-written, waiting for a native read), sound effects, Actions, and Export in Format.
- The local HTTP API and the command-line tool.

### Not there yet

- Some File ▸ New templates, normal and bump maps on 3D models, and a check on screen of some newer features (see [ROADMAP.md](https://github.com/AhmedKhan-GH/falcon/blob/main/ROADMAP.md)).
- Eagle's plugins.
- Windows and Linux, and signed releases.

## Running the docs

```sh
scripts/docs.sh
```

This builds the API docs with `cargo doc` and serves the book at <http://localhost:3000>, reloading on changes.

## Workspace

| Crate | Purpose |
|---|---|
| `falcon-library` | Reading and writing `.library` folders: import, thumbnails, palettes, metadata, editing, search, smart folders, duplicates, undo, backups, `.eaglepack` files and the sandbox check |
| `falcon-server` | Eagle's local HTTP API |
| `falcon-app` | The desktop app, built with egui |
| `falcon-video` | Probing, decoding and playing video through AVFoundation, falling back to `falcon-ffmpeg` |
| `falcon-ffmpeg` | The fallback video decoder and player over an LGPL FFmpeg opened at run time (see [Video playback](video.md)) |
| `falcon-pdf` | PDF pages, text, search and selection through PDFKit |
| `falcon` | Command-line tool: `new`, `import`, `folder`, `info`, `backup`, `export`, `import-pack`, `serve` |

Build a launchable `Falcon.app` with `scripts/bundle.sh`.
