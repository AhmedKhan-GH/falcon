# The desktop app

`falcon-app` is falcon's version of Eagle's main window, built with egui. Its look comes from Eagle itself:

- **Colours:** sampled from Eagle's window and its dark-theme stylesheet (`style/base/_color.scss`). For example, the content area is `#17191d`, the panels `#1f2023` and the accent `#3297ff`.
- **Fonts and icons:** SF Pro for text, plus Phosphor Light icons, which are MIT-licensed and close to Eagle's line icons.
- **Measurements:** taken from Eagle at 1280 × 800: 240 pt panels, a 48 pt toolbar, 27 pt sidebar rows and 139 pt cards.

## What it does

| Area | Features |
|---|---|
| Window | No title bar: the traffic lights sit over the sidebar, and empty bar space drags the window |
| Sidebar | Library switcher, Eagle's fixed entries (All, Uncategorized, Untagged, Recently Used, Random, All Tags, Trash), smart folders, and a folder tree with guide lines and direct-item counts. You can filter it, create, rename and delete folders, and drop items onto folders or the Trash |
| Toolbar | Back and forward, zoom slider, layout and sort menu, filter bar toggle, search with Eagle's syntax |
| Filter bar | Colour (presets, hex value, accuracy, grayscale), tags, folders, shape, rating, type, notes, URL |
| Grid | Square cards with type badges, the name and dimensions. Images are never enlarged. Supports multi-select, drag and a context menu |
| Inspector | Preview and palette (click a colour to search for it), editable title, notes and URL, tags, folders, rating and properties. With nothing selected it shows the folder's name, description and totals |
| Other | Full-window viewer, Empty Trash and permanent delete with confirmation, the embedded HTTP API, and the last library and layout reopening on launch |

## Running it

```sh
cargo run -p falcon-app -- path/to/Some.library
scripts/bundle.sh   # builds target/release/Falcon.app
```
