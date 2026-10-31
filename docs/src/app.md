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
| Toolbar | Back and forward, zoom slider, layout and sort menu, filter bar toggle, search with Eagle's syntax, plus typo tolerance (see Search and filters) |
| Filter bar | Colour (presets, hex value, accuracy, grayscale), tags, folders, shape, rating, type, notes, URL |
| Grid | Four layouts. **Waterfall** (the default): equal columns, each card keeping its image's shape. **Justified**: rows of equal height. **Grid**: square cards. **List**: rows showing type, size, dimensions and date. Cards have type badges. Supports multi-select, drag (inside the app, or out to Finder and other apps), a context menu and a hover preview (see below) |
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
| ⌘Z and ⇧⌘Z | Undo and redo library edits, up to 100 steps. Each frame's edits are one step, named after what was done ("Undid: Move 3 to Trash"). Deleting permanently or emptying the Trash can't be undone and clears the history. Text fields keep their own undo while focused |
| Hold Z | Show the hover preview of the card under the pointer |

## Saving

The app opens libraries in batched write mode (see [Write batching](library-format.md#write-batching)): edits made during a frame, including those from the HTTP API and background imports since the last frame, are written together at the end of the frame, and a metadata backup follows when `mtime.json` or `metadata.json` changed. Pending writes are also flushed when the app saves its state on quit. A failed write shows "Could not save".

## Hover preview

Hovering a card shows a faint zoom button in the bottom-right corner of its image (in the List layout, over the whole thumbnail). Resting on the button for 200 ms floats an enlarged copy of the image over the window, which dims behind it. Holding Z shows it at once for whichever card is under the pointer. The code is in `crates/falcon-app/src/hover.rs`.

What Eagle does, from its `js/hover-preview.js`, `style/components/_hover-preview.scss` and `_box-container.scss`:

| Question | Eagle | Seen or inferred | falcon |
|---|---|---|---|
| Trigger | Not the card itself: a zoom button (24 pt, 20 pt on small cards, growing on large ones, 20–32 pt) in the thumbnail's bottom-right corner, or the whole thumbnail in List. It fades in to half opacity 250 ms after the card is hovered, and to full when hovered. The `habits.hoverZoom` preference (on by default) hides it | Seen | Same, without the 250 ms fade-in and with no preference yet |
| Delay | 200 ms on the button (`defaultDelay`, overridable per element with `data-hover-delay`) | Seen | 200 ms |
| Key | Holding Z shows the preview at once, and moving onto other cards switches it. Ignored with ⌘, ⌃ or ⇧, in text fields and in the viewer. Releasing Z hides it | Seen | Same |
| Anchoring | Anchored to the card, worked out once when it shows; it does not follow the pointer | Seen | Anchored to the card, recomputed each frame |
| Placement | Picks above/below or left/right, whichever lets the image be largest; ties go beside the card unless the card is partly off screen. Beside, it starts 15 pt inside the card's near edge and may cover the card; above or below it is left-aligned with the card. Its top sits a quarter of the card (at most 20 pt) below the card's top. Then it is pushed back inside the window, 10 pt from the edges | Seen | Same rule, rewritten (`hover::preview_rect`) |
| Size | As large as the chosen area allows, never larger than the original's pixel size. An unused 480 pt cap appears in the code | Seen | Same; `preview_rect` takes an optional cap, unused |
| Image | Shows the thumbnail at once, then swaps in the original 50 ms later once it loads, for avif, gif, webp, png, jpg, jpeg, bmp, ico and jfif under 56 megapixels | Seen | Shows the grid thumbnail, then the original (decoded at up to 3072 px) for the same types when the preview is larger than the 512 px grid decode |
| Look | 3 pt frame in the panel border colours, 6 pt corners, a drop shadow (0 12 24, 30% black) and a 1 pt dark ring; the window behind dims to 50% black; fades in over about 150 ms | Seen | Same, with a uniform frame colour |
| Other types | Video, audio, font and web-link cards have no zoom button (video and audio have their own hover playback, a separate feature). Holding Z still shows their thumbnail | Seen | Same; hover playback is not built |
| Caption | None: only the image | Seen | None |
| Cancels | Leaving the button, leaving the card, releasing Z, clicking a card (a held Z must then be pressed again), opening the viewer, and the item list refreshing (search, filter, folder change) | Seen | Same, plus any pointer press, a drag, and scrolling (for the button; with Z held it switches to the card now under the pointer) |
| Scrolling | No explicit handler; the pointer leaving the card as the grid scrolls closes it | Inferred | Closes on scroll |
| `noPreview` | Checked before showing, but nothing in the bundle sets it | Inferred unused | Not modelled |

## Running it

```sh
cargo run -p falcon-app -- path/to/Some.library [ITEM…|FILE…]
scripts/bundle.sh   # builds target/release/Falcon.app
```

One item id (or an `eagle://item/<id>` link) opens that item in the viewer. Several ids select them. Files are imported.
