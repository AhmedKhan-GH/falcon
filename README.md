# Falcon

Falcon is a free, open-source asset manager for designers, written in Rust. It opens libraries in the [Eagle](https://eagle.cool) format directly, so the same `.library` folder works in both apps.

It is early alpha. The core workflow works today: importing, organising, searching, browsing and editing. The list under [Status](#status) shows what is still missing.

**Website:** <https://ahmedkhan-gh.github.io/falcon/>  ·  **Docs:** <https://ahmedkhan-gh.github.io/falcon/book/>

## Features

- **Reads and writes Eagle libraries.** Falcon reads `metadata.json`, the item files, the folder tree, tags and smart folders. Writing a library back gives byte-identical JSON.
- **Import:** drag and drop, paste with ⌘V, or files opened with the app. Falcon makes thumbnails and colour palettes the way Eagle does. PDF, video, PSD, AI, HEIC and RAW files go through macOS QuickLook. Imports run in the background.
- **Organise:**
  - folders: nested, drag to move, Quick Access
  - tags: rename, merge, star, edit across many items at once
  - smart folders with rule rows
  - rating, notes and URLs
  - Trash and Empty Trash
- **Search:**
  - keyword search with AND, OR, `-not`, quoted phrases and groups
  - a filter bar for colour, tags, folders, shape, rating, type, notes and URL
  - colour similarity search (CIEDE2000)
  - Eagle's sort orders
- **Browse:**
  - Waterfall, Justified, Grid and List layouts
  - a viewer with zoom, pan and animated GIFs
  - dragging items out into Finder and other apps
- **Developers:**
  - an Eagle-compatible local HTTP API (`falcon serve`, also built into the app)
  - a command-line tool
  - Rust crates for reading and writing libraries

## Build

You need Rust 1.89 or newer. The desktop app currently targets macOS.

```sh
git clone https://github.com/AhmedKhan-GH/falcon
cd falcon
cargo run --release -p falcon-app -- path/to/Some.library
scripts/bundle.sh        # builds target/release/Falcon.app
```

The command-line tool:

```sh
cargo run -p falcon -- info path/to/Some.library
cargo run -p falcon -- import path/to/Some.library photo.jpg --tag inspiration
cargo run -p falcon -- serve path/to/Some.library   # HTTP API on 127.0.0.1:41596
```

## Workspace

| Crate | Purpose |
|---|---|
| `falcon-library` | Reading and writing libraries: import, thumbnails, palettes, editing, search, smart folders |
| `falcon-server` | The local HTTP API |
| `falcon-app` | The desktop app, built with egui |
| `falcon` | Command-line tool |

`docs/` is an mdBook explaining how each part works, including what was observed in Eagle's behaviour and where falcon deliberately differs. Serve it locally with `scripts/docs.sh`.

## Status

These are not done yet:
- **Viewer:** video and audio playback.
- **Import:** a browser extension and web importers.
- **Other:** plugins, sync between devices, and AI search.
- **Platforms:** Windows and Linux builds.

Back up a library before trying falcon on it, and don't edit the same library in Eagle and falcon at the same time.

## About Eagle

Falcon is an independent project and is not affiliated with, endorsed by or sponsored by Eagle or its developers. "Eagle" is a trademark of its respective owner and is used here only to describe file-format compatibility. Falcon contains no Eagle code or assets. Its behaviour was worked out by studying how the Eagle app behaves and what it writes to disk.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution you submit for inclusion in this work, as defined in the Apache-2.0 license, is dual licensed as above, without any additional terms or conditions.
