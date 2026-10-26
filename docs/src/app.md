# The desktop app

`falcon-app` is falcon's version of Eagle's main window, built with egui. Its look comes from Eagle itself:

- **Colours:** sampled from Eagle's window and its dark-theme stylesheet (`style/base/_color.scss`). For example, the content area is `#17191d`, the panels `#1f2023` and the accent `#3297ff`.
- **Fonts and icons:** SF Pro for text, plus Phosphor Light icons, which are MIT-licensed and close to Eagle's line icons.
- **Measurements:** taken from Eagle at 1280 × 800: 240 pt panels, a 48 pt toolbar, 27 pt sidebar rows and 139 pt cards.

## What it does

| Area | Features |
|---|---|
| Window | No title bar: the traffic lights sit over the sidebar, and empty bar space drags the window |
| Library menu | Recent libraries, plus Open, Create and Show in Finder. The last library, layout and sort reopen on launch |
| Sidebar | Eagle's fixed entries, Quick Access, smart folders, and a folder tree with guide lines and direct-item counts. You can filter it, create, rename, delete and drag-move folders, and drop items onto folders or the Trash |
| Toolbar | Back and forward, zoom slider, layout and sort menu, filter bar toggle, search with Eagle's syntax |
| Filter bar | Colour (presets, hex value, accuracy, grayscale), tags, folders, shape, rating, type, notes, URL |
| Grid | Square cards with type badges, the name and dimensions. Images are never enlarged. Supports multi-select, drag and a context menu |
| Inspector | **One item:** preview, palette (click a colour to search for it), editable title, notes and URL, tags, folders, rating and properties. **Several items:** a preview stack, shared tags, add a tag to all, add all to a folder, a shared rating, trash all |
| Viewer | Zoom around the pointer, pan, fit (0), actual size (1), + and −, animated GIFs, and full-size QuickLook renders |
| All Tags | A tag manager with a filter, starred tags, and rename, merge and delete |
| Import | Drag and drop, ⌘V (files or images), and files passed to the app. Imports run in the background with a progress bar. PDF, video, PSD, AI, HEIC and RAW go through QuickLook |
| Smart folders | Create and edit them with rule rows, matched by any or all |
| Trash | Restore, Delete Permanently and Empty Trash, all with confirmation |
| HTTP API | Runs while the app is open, from port 41596 up |

## Keys

| Key | Action |
|---|---|
| Space | Open or close the viewer |
| ← → ↑ ↓ | Move the selection, or go to the next item in the viewer |
| ⌘A | Select all |
| ⌘⌫ | Move to the Trash, or delete permanently when in the Trash |
| F2 | Rename the selected item |
| ⌘F | Focus search |
| ⌘[ and ⌘] | Back and forward |
| ⌘= and ⌘− | Zoom the grid |
| ⌘⇧N | New folder |
| ⌘C and ⌘V | Copy the selected files, or paste files or images |

## Running it

```sh
cargo run -p falcon-app -- path/to/Some.library [ITEM…|FILE…]
scripts/bundle.sh   # builds target/release/Falcon.app
```

One item id (or an `eagle://item/<id>` link) opens that item in the viewer. Several ids select them. Files are imported.
