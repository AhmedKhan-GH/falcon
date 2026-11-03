<p align="center">
  <img src="docs/brand/falcon-icon-256.png" width="128" height="128" alt="Falcon app icon">
</p>

<h1 align="center">Falcon</h1>

<p align="center">
  <b>An open-source asset manager for everyone, written in pure Rust.</b><br>
  Collect, tag, search and browse images, PDFs, fonts and videos in one library,<br>
  in a native app that is free, offline and yours.
</p>

<p align="center">
  <img alt="100% Rust" src="https://img.shields.io/badge/100%25-Rust-b7410e?style=flat-square&logo=rust">
  <img alt="macOS" src="https://img.shields.io/badge/macOS-native-2f7bf5?style=flat-square&logo=apple">
  <img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-3a3a3a?style=flat-square">
  <img alt="Status: early alpha" src="https://img.shields.io/badge/status-early%20alpha-d69e2e?style=flat-square">
</p>

<p align="center">
  <a href="https://ahmedkhan-gh.github.io/falcon/"><b>Website</b></a> ·
  <a href="https://ahmedkhan-gh.github.io/falcon/book/">Documentation</a> ·
  <a href="ROADMAP.md">Roadmap</a> ·
  <a href="FORMATS.md">Formats</a> ·
  <a href="PLUGINS.md">Plugins</a>
</p>

<br>

<p align="center">
  <img src="docs/images/falcon-hero.jpg" alt="Falcon showing a library of public-domain paintings in the Waterfall layout, with Seurat's A Sunday on La Grande Jatte selected and its palette, notes, tags and properties in the inspector" width="100%">
  <br>
  <sub>A library of public-domain paintings in the Waterfall layout, with the inspector open on the selected work.<br>
  <i>A Sunday on La Grande Jatte — 1884</i>, Georges Seurat, 1884–86</sub>
</p>

<p align="center">
  <a href="#features">Features</a> ·
  <a href="#get-started">Get started</a> ·
  <a href="#under-the-hood">Under the hood</a> ·
  <a href="#status">Status</a> ·
  <a href="#license">License</a>
</p>

<br>

<table>
  <tr>
    <td width="25%" valign="top">
      <h3>📥 Collect anything</h3>
      Drag files in, paste with ⌘V, or open files with the app. Thumbnails and colour palettes are made for you. Falcon handles exactly Eagle's set of file formats, listed in [FORMATS.md](FORMATS.md). PDF, PSD, AI, HEIC, RAW and video come in with thumbnails from macOS QuickLook where it helps, and screen capture is built in.
    </td>
    <td width="25%" valign="top">
      <h3>🗂️ Organise your way</h3>
      Nested folders, tags, smart folders, ratings, notes and Quick Access. Edit tags across hundreds of items at once, and rename or merge tags across the whole library.
    </td>
    <td width="25%" valign="top">
      <h3>🔎 Find it again</h3>
      Search by keyword with AND, OR and NOT, filter by colour, shape, type and rating, or find images by colour similarity.
    </td>
    <td width="25%" valign="top">
      <h3>⚡ Native and fast</h3>
      Written in Rust with egui. No Electron; only live web pages use the system web view. Thumbnails decode on worker threads, and 40 large images import in under three seconds.
    </td>
  </tr>
</table>

<br>

## Features

Every screenshot here is the real app at work on public-domain art. The credits are in [ATTRIBUTION.md](ATTRIBUTION.md).

<table>
  <tr>
    <td width="50%" valign="top">
      <img src="docs/images/falcon-color.jpg" alt="The filter bar with a colour filter showing four sea paintings" width="100%">
      <br>
      <sub>A colour filter finding the sea paintings.</sub>
      <h3>Search by colour</h3>
      Every image gets a dominant-colour palette when it is imported. Pick a colour, or click a swatch in the inspector, to find images with that colour, sorted by how close they are (CIEDE2000). Combine it with tags, folders, shape, rating, type, notes and URL filters.
    </td>
    <td width="50%" valign="top">
      <img src="docs/images/falcon-viewer.jpg" alt="Monet's The Beach at Sainte-Adresse open in the viewer, between the sidebar and the inspector" width="100%">
      <br>
      <sub>The viewer, between the sidebar and the inspector.<br><i>The Beach at Sainte-Adresse</i>, Claude Monet, 1867</sub>
      <h3>A viewer that keeps up</h3>
      Zoom around the pointer, pan, fit or show actual size, and step through the library with the arrow keys. Animated GIFs play, and PDFs, PSD and AI files render at full resolution. Video (with an FFmpeg fallback that includes AV1), audio, fonts, text, 3D models with textures, IFC and web pages have viewers too.
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <img src="docs/images/falcon-batch.jpg" alt="Three Monet paintings selected, with the inspector showing their shared tags and actions that apply to all of them" width="100%">
      <br>
      <sub>Editing three selected works at once.</sub>
      <h3>Edit in bulk</h3>
      Select many items to add or remove tags, file them into a folder, rate them or move them to the Trash in one step. Drag items onto any folder, or out of the window into Finder, Figma or Photoshop.
    </td>
    <td width="50%" valign="top">
      <img src="docs/images/falcon-tags.jpg" alt="The tags page listing every tag with its item count, grouped by letter" width="100%">
      <br>
      <sub>The tags page.</sub>
      <h3>Tags you can manage</h3>
      See every tag with its count, star the ones you use most, and rename, merge or delete tags across the library. Smart folders gather items that match rules on name, tags, type, colour, rating, size or date.
    </td>
  </tr>
  <tr>
    <td colspan="2" valign="top">
      <img src="docs/images/falcon-justified.jpg" alt="Paintings arranged in the Justified layout, in rows of equal height" width="100%">
      <br>
      <sub>The Justified layout.</sub>
      <h3>Four ways to browse</h3>
      <b>Waterfall</b> keeps each image's shape in balanced columns, <b>Justified</b> fills rows edge to edge, <b>Grid</b> shows tidy square cards, and <b>List</b> lists type, size, dimensions and dates. The zoom slider works in every layout.
    </td>
  </tr>
</table>

## Get started

You need Rust 1.89 or newer. The desktop app currently targets macOS.

```sh
git clone https://github.com/AhmedKhan-GH/falcon
cd falcon
cargo run --release -p falcon-app          # choose or create a library on first launch
scripts/bundle.sh                          # build target/release/Falcon.app
```

While falcon is in development, the app and the command-line tool only open libraries inside `~/Falcon Sandbox` (set `FALCON_SANDBOX` to use another folder). Copy a library there to try falcon on it.

```sh
cargo run -p falcon -- new ~/"Falcon Sandbox"/Inspiration.library
cargo run -p falcon -- import ~/"Falcon Sandbox"/Inspiration.library photo.jpg --tag moodboard
cargo run -p falcon -- info ~/"Falcon Sandbox"/Inspiration.library
cargo run -p falcon -- serve ~/"Falcon Sandbox"/Inspiration.library     # local HTTP API on 127.0.0.1:41596
```

## Under the hood

| Crate | What it does |
|---|---|
| `falcon-library` | The library format: loading and byte-identical saving, import, thumbnails (WebP), colour palettes (median cut), editing, search, smart folders |
| `falcon-server` | A local JSON HTTP API for scripts and tools, also built into the app |
| `falcon-video` | Video playback and frame decoding through macOS's AVFoundation, falling back to `falcon-ffmpeg`, used by the app's viewer |
| `falcon-ffmpeg` | The fallback video decoder and player over an LGPL FFmpeg opened at run time |
| `falcon-pdf` | PDF pages, text, search and selection through macOS's PDFKit, used by the app's PDF reader |
| `falcon-clip` | AI Search: on-device CLIP embeddings through candle, the model's download, the index and its worker |
| `falcon-app` | The desktop app, drawn with egui |
| `falcon` | The command-line tool |

- **Plain files:** a library is a folder of plain files: one `metadata.json` per item, next to the original file and its thumbnail. There is no database to corrupt, and you can back a library up with any copy tool.
- **Eagle libraries:** falcon reads and writes the Eagle library format, so existing Eagle libraries open in falcon unchanged.
- **Documentation:** [the book](https://ahmedkhan-gh.github.io/falcon/book/) explains each part, and [the API docs](https://ahmedkhan-gh.github.io/falcon/book/api/falcon_library/index.html) cover the crates.

## Status

Falcon is early alpha and unsigned: the everyday workflow works, about 99% of Eagle's local features are done, and the [roadmap](ROADMAP.md) lists what is left. [PARITY.md](PARITY.md) checks every feature against Eagle 4.0.0, [FORMATS.md](FORMATS.md) lists the file formats and [PLUGINS.md](PLUGINS.md) lists Eagle's plugins, which falcon does not have; Actions and Export in Format are built in. Back up a library before trying it, and don't edit the same library in two apps at once.

Falcon is an independent project. It is not affiliated with, endorsed by or sponsored by Eagle or its developers. "Eagle" is a trademark of its respective owner and appears here only to describe file-format compatibility.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option. Unless you explicitly state otherwise, any contribution you submit for inclusion in this work, as defined in the Apache-2.0 license, is dual licensed as above, without any additional terms or conditions.
