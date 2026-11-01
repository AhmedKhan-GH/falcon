# Introduction

Falcon is a rebuild of the [Eagle](https://eagle.cool) asset manager in Rust, written to learn how Eagle works.

Each chapter in the first part describes one piece of Eagle. It covers what Eagle does, with every claim marked **observed** or **inferred**, and then how falcon implements it.

- **Observed** claims come from Eagle's own files: a real library on disk, or REA analysis of `app.asar`.
- **Inferred** claims are conclusions drawn from those files that haven't been checked directly.

Falcon is written from that behaviour. It doesn't copy Eagle's code.

The second part, [The desktop app](app.md), describes falcon's desktop app: its window, viewers, keys, preferences, themes and how it opens and saves libraries.

## Status

Falcon is early alpha. [PARITY.md](https://github.com/AhmedKhan-GH/falcon/blob/main/PARITY.md) compares it with Eagle 4.0.0 feature by feature, checked against falcon's code; about a third of Eagle's local features are done, about a sixth are partly done and the rest are missing. [ROADMAP.md](https://github.com/AhmedKhan-GH/falcon/blob/main/ROADMAP.md) lists what comes next.

While falcon is in development, the app and the CLI only open libraries inside `~/Falcon Sandbox` (or the folder in `FALCON_SANDBOX`). To work on an existing library, copy it there first. See [Sandbox](app.md#sandbox).

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
| `falcon-video` | Probing, decoding and playing video through AVFoundation |
| `falcon-pdf` | PDF pages, text, search and selection through PDFKit |
| `falcon` | Command-line tool: `new`, `import`, `folder`, `info`, `backup`, `export`, `import-pack`, `serve` |

Build a launchable `Falcon.app` with `scripts/bundle.sh`.
