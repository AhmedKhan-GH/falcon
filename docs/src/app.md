# The desktop app

`falcon-app` is falcon's version of Eagle's main window, built with egui. Its look comes from Eagle itself:

- **Colours:** sampled from Eagle's window and its dark-theme stylesheet (`style/base/_color.scss`). For example, the content area is `#17191d`, the panels `#1f2023` and the accent `#3297ff`.
- **Fonts and icons:** SF Pro for text, plus Phosphor Light icons, which are MIT-licensed and close to Eagle's line icons.
- **Measurements:** taken from Eagle at 1280 × 800: 240 pt panels, a 48 pt toolbar, 27 pt sidebar rows and 139 pt cards.

## What it does

| Area | Features |
|---|---|
| Window | No title bar: the traffic lights sit over the sidebar, and empty bar space drags the window |
| Library menu | Recent libraries, plus Open, Create, Show in Finder, the collect window and Preferences. The last library, layout and sort reopen on launch |
| Preferences | ⌘, or the library menu (see below) |
| Sidebar | Eagle's fixed entries, Quick Access, smart folders, and a folder tree with guide lines and direct-item counts. You can filter it, create, rename, delete and drag-move folders, and drop items onto folders or the Trash |
| Toolbar | Back and forward, zoom slider, layout and sort menu, filter bar toggle, search with Eagle's syntax, plus typo tolerance (see Search and filters) |
| Filter bar | Colour (presets, hex value, accuracy, grayscale), tags, folders, shape, rating, type, notes, URL |
| Grid | Four layouts. **Waterfall** (the default): equal columns, each card keeping its image's shape. **Justified**: rows of equal height. **Grid**: square cards. **List**: rows showing type, size, dimensions and date. Cards have type badges. Supports multi-select, drag (inside the app, or out to Finder and other apps), a context menu and a hover preview (see below) |
| Inspector | **One item:** preview, palette (click a colour to search for it), editable title, notes and URL, tags, folders, rating and properties. **Several items:** a preview stack, shared tags, add a tag to all, add all to a folder, a shared rating, trash all |
| Viewer | Zoom around the pointer, pan, fit (0), actual size (1), + and −, animated GIFs, and full-size QuickLook renders |
| All Tags | A tag manager with a filter, starred tags, and rename, merge and delete |
| Import | Drag and drop, ⌘V (files or images), files passed to the app, and the collect window (see below). Imports run in the background with a progress bar. PDF, video, PSD, AI, HEIC and RAW go through QuickLook |
| Smart folders | Create and edit them with rule rows, matched by any or all |
| Trash | Restore, Delete Permanently and Empty Trash, all with confirmation |
| HTTP API | Runs while the app is open, from port 41596 up |

## Keys

These are the defaults. All but the arrows, Esc and Z can be changed under Preferences → Shortcuts (see Shortcut settings).

| Key | Action |
|---|---|
| Space (or ↩) | Open or close the viewer |
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
| ⌘⇧F | Show or hide the filter bar |
| ⌘⌥1 and ⌘⌥2 | Show or hide the sidebar and the inspector |
| ⌥1 to ⌥4 | Grid, Justified, Waterfall and List layouts |
| Esc | Close the viewer, or clear the selection |
| Hold Z | Show the hover preview of the card under the pointer |
| ⌘⇧K | Show or hide the collect window |
| ⌘, | Preferences |

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

## Preferences

⌘, (or Preferences… in the library menu) opens a window over the main one, laid out like Eagle's: sections on the left, the section on the right, and Save, Apply and Cancel at the bottom. Edits stay in a draft until Save (also ⌘↩) or Apply; Cancel, the close button and Esc drop them, as in Eagle. The code is in `crates/falcon-app/src/prefs.rs`, and the values are saved with the rest of the app state.

| Section | Setting | Eagle | falcon |
|---|---|---|---|
| General | Show the sidebar, show the inspector | View menu toggles, not preferences | Same flags as the panel buttons |
| General | Add IPTC and XMP keywords as tags | `general.IPTC`, off by default | Stored as `keywords_as_tags`; import does not read it yet |
| General | Back up metadata on change | Eagle backs up on its own; no preference | Stored as `backup_on_change`; nothing calls it yet |
| Preview | Hover zoom | `habits.hoverZoom`, on by default; off removes the zoom button, but holding Z still works | Same |
| Developer | Run the HTTP API, first port | Eagle always serves on 41595 and only has an API token setting | On by default, from 41596; tries five ports. Ports whose range would include 41595, or below 1024, are refused. Changing either restarts the API |

What Eagle does, from `js/default-preferences.js`, `preferences.html`, `js/preferences.js` and `css/preferences.css` (all seen):

- Preferences live in one tree with groups `general`, `habits`, `shortcuts.keybinds`, `theme`, `notification`, `sidebar`, `screencapture`, `video`, `proxy`, `privacy`, `autoImport` and `developer`. Booleans are stored as the strings `'true'` and `'false'`, and choices as strings (`hoverZoom: "on"`).
- The window's sidebar (200 pt wide, 32 pt rows) has General and Sidebar, then Controls, Preview (the `habits` group), Screenshot and Shortcuts, then Notification, Password and Auto-Import, then the AI panels. A search box filters every panel by keywords.
- Save applies and closes, Apply applies and stays open, Cancel and Esc revert (including a previewed theme or zoom), ⌘↩ saves.
- The window's own size is set in Eagle's main process, which is not in the readable bundle; falcon's 720 × 500 pt is a guess (inferred).
- Not built in falcon: themes, interface zoom, language, the sidebar item toggles, scroll and double-click habits, notifications, screenshots, password lock, auto-import, proxy and the preference search.

## Shortcut settings

Preferences → Shortcuts lists every rebindable command by group, with its chord. Clicking a chord starts recording: the next key press becomes the shortcut. Backspace alone clears it, Esc alone stops recording, and a letter, digit or symbol needs a modifier (function keys, Space, ↩, arrows, Home, End, Page Up/Down, Delete and brackets may stand alone). A chord another command already uses is refused with "Used by …" for 1.5 s, and the old chord stays. A changed row has a reset button, and Restore Defaults (after a confirmation) resets them all. The table, the parser and the recorder rules are in `crates/falcon-app/src/shortcuts.rs`.

Changes are saved as a map from command id to Eagle's accelerator text, for example `{"edit.rename": "Command + R"}`, holding only the changed commands; an empty string means unbound. Ids reuse Eagle's keybind names where Eagle has the command (`file.create.folder`, `view.zoom.in`, `find.filter.toggle`, `app.preferences`). When a key press matches several chords (⌘F and ⌘⇧F for ⌘⇧F), the one with the most modifiers wins. ⌘C, ⌘X and ⌘V reach egui as Copy, Cut and Paste events rather than key presses, so falcon maps them back; this also makes ⌘C in the grid work, which the old hard-coded check missed.

What Eagle does, from `js/default-preferences.js`, `js/preferences.js`, `js/services/shortcut-manager.js` and `js/directives/shortcut-input.js` (all seen):

- `shortcuts.keybinds` maps 150 names (`file.create.folder`, `view.zoom.in`, …) to Electron accelerators such as `CmdOrCtrl + Shift + N`. On load, `CmdOrCtrl` is rewritten to `Command` (macOS) or `Ctrl`. Platform pairs such as `edit.rename.darwin` (⌘R) and `edit.rename.win32` (F2) show only on their platform.
- The recorder writes modifiers in the order Ctrl, Command, Shift, Alt, then the key; letters and digits need a modifier; Backspace alone clears.
- Conflicts are checked within a group's `conflict` scope: player keys only clash with other player keys, everything else with everything else. A clashing chord is refused, the old one stays, and a note names the other command for 1.5 s.
- Restore Defaults asks first, then reloads the defaults (plugin shortcuts too). The window has a shortcut search box; falcon has no search.
- falcon keeps F2 for rename (Eagle's macOS default is ⌘R) and its own ids for commands Eagle lacks or names differently (`view.viewer`, `go.back`, `edit.select.all`) (inferred mapping).

## Collect window

⌘⇧K, or Show Collect Window in the library menu, opens a small (280 × 360 pt) borderless window that floats above all other apps. Files dropped on it, or pasted into it with ⌘V (copied files or image data), go through the normal background import into the folder picked in its list ("No folder" by default; a search box narrows the list by any part of a folder's path). The choice is saved with the preferences, and a folder that is missing from the open library counts as no folder. Drag the header to move it; ×, Esc or ⌘⇧K close it. Its position is remembered between launches. The code is in `crates/falcon-app/src/collect.rs`.

It is a second OS window (an egui immediate viewport with `with_always_on_top` and `with_decorations(false)`). In eframe 0.37 each viewport has its own winit window and input, and `WindowEvent::DroppedFile` lands in that viewport's `raw.dropped_files`, so drops work on it directly; no fallback was needed. On a backend without multiple viewports egui draws it as a panel inside the main window instead, and drops there go to the main window's import.

What Eagle's collect window is, from `collect-window/` and `js/background.js` (seen):

- It is not a drop target. It opens for one incoming item, when the browser extension adds an image (`general.showCollectModal`, "ask me where to save", off by default, also switchable through the API) or after a screenshot (`general.showCaptureCollectModal`).
- Left side (240 pt): the image preview with its dimensions, a 1–5 rating, an editable title and description, and tags. Right side: a searchable folder list with recent folders first, multi-select and creating a folder; choosing a folder saves the item there with the folder's auto-tags and closes the window.
- The window is frameless with 8 pt rounded corners on a translucent background. Its size, position and always-on-top setting are set in Eagle's main process, which is not in the readable bundle (inferred: small, centred, on top).

falcon keeps the folder chooser and the always-on-top window but works as a drop target, because falcon has no browser extension or screenshots yet. Not built: per-item title, description, rating and tags, recent folders, multi-select, creating a folder from the window, and dropping images straight from a browser (macOS hands winit only file drops, so browser image drags that carry no file are ignored).

## Running it

```sh
cargo run -p falcon-app -- path/to/Some.library [ITEM…|FILE…]
scripts/bundle.sh   # builds target/release/Falcon.app
```

One item id (or an `eagle://item/<id>` link) opens that item in the viewer. Several ids select them. Files are imported.
