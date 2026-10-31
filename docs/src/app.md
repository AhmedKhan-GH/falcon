# The desktop app

`falcon-app` is falcon's version of Eagle's main window, built with egui. Its look comes from Eagle itself:

- **Colours:** sampled from Eagle's window and its dark-theme stylesheet (`style/base/_color.scss`). For example, the content area is `#17191d`, the panels `#1f2023` and the accent `#3297ff`.
- **Fonts and icons:** SF Pro for text, plus Phosphor Light icons, which are MIT-licensed and close to Eagle's line icons.
- **Measurements:** taken from Eagle at 1280 × 800: 240 pt panels, a 48 pt toolbar, 27 pt sidebar rows and 139 pt cards.

## What it does

| Area | Features |
|---|---|
| Window | No title bar: the traffic lights sit over the sidebar, and empty bar space drags the window |
| Library menu | The open libraries (each with a close button), then recent ones, then Open, New, Show in Finder, the collect window and Preferences. Several libraries can be open at once (see Several libraries). The open libraries, layout and sort reopen on launch |
| Preferences | ⌘, or the library menu (see below) |
| Sidebar | Eagle's fixed entries, Quick Access, a nested smart folder tree, and a folder tree with guide lines and direct-item counts. Folders and smart folders show their icon in their colour, and locked folders a lock. You can filter it, create, rename and delete folders, drag folders and smart folders to reorder or nest them, and drop items onto folders or the Trash (see Organising below) |
| Toolbar | Back and forward, zoom slider, layout and sort menu, filter bar toggle, search with Eagle's syntax, plus typo tolerance (see Search and filters) |
| Filter bar | Colour (presets, hex value, accuracy, grayscale), tags, folders, shape, rating, type, notes, URL, date, duration, camera and font (see below), and saved filters |
| Grid | Four layouts. **Waterfall** (the default): equal columns, each card keeping its image's shape. **Justified**: rows of equal height. **Grid**: square cards. **List**: rows showing type, size, dimensions and date. Cards have type badges. Supports multi-select, drag (inside the app, or out to Finder and other apps), a context menu and a hover preview (see below) |
| Inspector | **One item:** preview, palette (click a colour to search for it), editable title, notes and URL, tags, folders, comments (edit and delete; click a region comment to show it), rating and properties (with duration, camera facts from `rawMetas`, and font family, style, PostScript name, version and glyph count when the item has them). **Several items:** a preview stack, shared tags, add a tag to all, add all to a folder, a shared rating, trash all |
| Viewer | Zoom around the pointer, pan, fit (0), actual size (1), + and −, animated GIFs, and full-size QuickLook renders. Audio plays over its waveform and fonts show a live specimen; images take region comments, rotate and crop (see below). Video doesn't play yet: the viewer shows its thumbnail |
| Card menu | Besides Open, Show in Finder and the folder and Trash entries: Set as Folder Cover (inside a folder), Set Custom Thumbnail… and Reset Thumbnail, Refresh Thumbnail, Refresh Palette, Rotate Left and Right, and Crop… (opens the viewer with the crop tool) |
| Folder view | Cards for the folder's subfolders above its items, each with its cover, name and counts. Click one to open it |
| All Tags | A tag manager with a filter, starred tags, rename, merge and delete, grouped A-Z or by tag group |
| Duplicates | Finds identical files and similar pictures, grouped, with Select All but First and Move to Trash (see below) |
| Import | Drag and drop, ⌘V (files or images), files passed to the app, and the collect window (see below). Imports run in the background with a progress bar. PDF, video, PSD, AI, HEIC and RAW go through QuickLook |
| Smart folders | Create and edit them with rule rows, matched by any or all, inside another smart folder, with an icon and colour |
| Trash | Restore, Delete Permanently and Empty Trash, all with confirmation |
| HTTP API | Runs while the app is open, from port 41596 up |

## Date, duration, camera and font filters

Four chips follow the existing ones in the filter bar. They set the `imported`, `modified`, `created`, `duration`, `cameras` and `font` rules of the library's `Filter` (see [Search and filters](search.md)), so the grid applies them like any other rule. The code is in `crates/falcon-app/src/app/filters_ui.rs`, with the parsing and labels in `logic.rs`.

What Eagle does, from `js/directives/filter-item-import.html`, `filter-item-mtime.html`, `filter-item-duration.html`, `filter-item-camera.html` and `filter-item-fonts.html`:

| Question | Eagle | Seen or inferred | falcon |
|---|---|---|---|
| Which chips show | Every filter item is hidden unless it is active or pinned (`hide-filter` when `!isEnabled && !pinned[type]`); pinning is a user choice | Seen | Date always shows. Duration, Camera and Font show only when the library has an item they can match (a `duration`, a camera, a font) or they are active. There is no pinning |
| Date | Two separate items, Date Imported and Date Modified. Each menu has the six presets (Today, Yesterday, Last 7, 30, 90 and 365 days) with counts, then every month that has items (`YYYY-MM`, newest first, with counts), then a Range checkbox that opens a calendar picker | Seen | One Date chip with a tab for Imported, Modified and Created (Created is falcon's own; Eagle only has it in smart folders). Each tab has the presets with counts, a range typed as two days (`2026-01-01` – `2026-12-31`, swapped if reversed, both needed), and the month list with counts. There is no calendar widget |
| Duration | Min and max text fields and a unit (seconds, minutes, hours) | Seen | Same, and a field also accepts a time such as `1:30`. The placeholders show the library's shortest and longest duration. The chip reads `0:30 – 2:00`, `≥ 0:30` or `≤ 2:00` |
| Camera | Every distinct `rawMetas.camera` value with its count; several can be ticked; "no results" when there are none | Seen | Same, counting items not in the Trash |
| Font | Two exclusive options, Activated and Deactivated, each with its count and shown only when its count is above 0 | Seen | Both options always listed with counts. "Activated" means the font's `<postScriptName>_.<ext>` key is among the files in `~/Library/Fonts/EagleApp/` (`installed_font_keys`), read once per session. On a Mac where Eagle never activated a font that folder is missing or empty, so every font counts as deactivated. falcon can't activate fonts itself |
| Clearing | A clear button on each item, and right-click | Seen | A Clear button inside each menu, and the bar's Clear for everything |

## Audio and font viewer

Opening an audio item (mp3, wav, flac, ogg, aac, m4a) shows its waveform with playback controls instead of an image; opening a font shows a specimen drawn with the font itself. The code is in `crates/falcon-app/src/audio.rs` (playback and the waveform cache) and `crates/falcon-app/src/app/media_view.rs` (drawing), with the seek and time arithmetic in `logic.rs`.

What Eagle does, from `js/directives/audio-media-element.js`, its video.js setup in `app.bundle.js`, `js/default-preferences.js` and `font-viewer/`:

| Question | Eagle | Seen or inferred | falcon |
|---|---|---|---|
| Player | video.js controls (play, volume, time, progress, loop, playback rate) over an HTML media element; wavesurfer draws the waveform inside the progress bar, 40 px high, unplayed `#7C7C7C`, played and cursor `#0072EF`, normalised | Seen | A waveform 160 pt high in the same two colours, normalised, with a playhead; under it a play/pause button, elapsed / total time, loop and volume. No playback rate |
| Start | Plays as soon as the item opens; loop is on | Seen | Same |
| Seeking | Click on the progress bar; ⌘← and ⌘→ step by 2% of the length, at most 10 s; ⌘↑ and ⌘↓ change the volume by 5% | Seen | Click or drag on the waveform. No step or volume keys, because ← and → already move to the next item |
| Space | Bound to `player.playAndPause` | Seen | Plays and pauses while an audio item is in the viewer; elsewhere it still opens and closes the viewer |
| Volume | Remembered in local storage, 100% by default | Seen | 100% at launch, kept until quit |
| Waveform data | Decoded in the page each time, nothing cached on disk | Seen | Peaks computed once per item on a background thread with `waveform::peaks` and kept in memory for the session. "Reading waveform…" shows meanwhile |
| Audio output | Chromium | Seen | rodio on the default output device, opened on first play and kept open. Only an output stream is opened, which macOS allows without a permission prompt; no input device is ever touched |
| Font viewer | A separate page with tabs for alphabet, article, waterfall (72 down to 12 px, edits mirrored on every line) and font information; a size slider from 31.25 to 625; editable text; light, dark and colour themes; an Activate button | Seen | One scrolling page: the sample text, editable in place, at a size from 12 to 200 pt; a waterfall of the same text from 72 down to 12 pt; and the character set. Font information is in the inspector. No themes, no activation |
| Rendering | Chromium with the font loaded as a web font | Seen | The font file (WOFF unpacked to sfnt; the first face of a collection) is added to egui's fonts under its own family the first time it is viewed, and stays for the session. Fonts egui's parser rejects, and WOFF2, fall back to the image viewer showing the specimen thumbnail |
| Sample text | Translated sample paragraphs per language | Seen | falcon's own short sample, in the script the font supports (Latin, Chinese, Japanese or Korean, chosen like the specimen thumbnail). Text you type stays as you move between fonts until Reset Text |

## Duplicates

The sidebar's Duplicates entry (after All Tags) opens a page that scans the library on a background thread, with a progress bar, and lists the groups it finds. Each group is a row of thumbnails labelled "Identical files" or "Similar pictures (within N of 64 bits)"; each thumbnail shows its size (identical) or its distance in bits from the first (similar), and the first is marked "Kept". Clicking selects as in the grid (⌘ and ⇧ work, and the inspector follows), double-clicking opens the viewer. **Select All but First** selects every member except the first of each group (never an item that is the first of another group), and **Move N to Trash** trashes the selection through the usual trash action, so ⌘Z undoes it. The similarity slider sets how many of the 64 perceptual-hash bits may differ (10 by default, right is stricter) and scans again. The code is in `crates/falcon-app/src/app/duplicates_view.rs`, with the grouping rules in `logic.rs`.

The scan fills the hash cache item by item so it can report progress (content hashes only for files that share their size with another), then calls the library's `find_exact_duplicates_cached` and `find_near_duplicates_cached` (see [Import](import.md)) on a copy of the library taken when the scan starts. Similar groups that only repeat an identical group are left out. The cache is kept in memory for the session, keyed by item id and checked against the original's size and mtime, so a second scan only hashes new or changed files. Nothing is written to the library or next to it.

What Eagle does, from `js/directives/duplicate-scan-panel.html` and `.js` and the import menu in `app.bundle.js`:

| Question | Eagle | Seen or inferred | falcon |
|---|---|---|---|
| Where | The import (+) menu's Find Duplicates submenu: the whole library, the current list, or the selected items | Seen | A sidebar entry that scans the whole library |
| Methods | Two separate scans to choose from first: identical files and similar images, each with its own progress bar (`current/total` and a percentage) | Seen | One scan does both, with the phase and `done/total` |
| Similarity | A slider (default 0.8) over a fingerprint weighting, re-grouping without rescanning | Seen | A bit threshold on the 64-bit pHash; changing it rescans, which is fast once hashes are cached |
| Results | Groups of thumbnails with name, size and dimensions; each group has a chosen item to keep (the first by default) and a checkbox; groups can be removed from the list | Seen | Groups as rows; the first is kept; no per-group checkbox or choosing another item to keep, and no removing a group from the list |
| Action | Merge: the kept item takes the group's combined tags, folders and notes (a merge editor lets you pick name, URL and notes), and the others go | Seen (merge); inferred (the others go to the Trash) | No merge: Move to Trash only, leaving the kept item's tags and folders as they were |
| Stored hashes | None in item JSON | Seen | None; an in-memory cache for the session |

## Keys

These are the defaults. All but the arrows, Esc and Z can be changed under Preferences → Shortcuts (see Shortcut settings).

| Key | Action |
|---|---|
| Space (or ↩) | Open or close the viewer. With audio in the viewer, Space plays or pauses instead (Esc or ↩ closes) |
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
| ⌘L | Open the library menu (Eagle's `library.switch`) |
| ⌘⇧[ and ⌘⇧] | Previous and next open library (falcon's own) |
| ⌘, | Preferences |
| C | In the viewer, turn comment mode on or off (Esc also leaves it) |
| ⇧R | In the viewer, rotate the image left |
| Enter and Esc | While cropping, save (then confirm) or cancel |

## Saving

The app opens libraries in batched write mode (see [Write batching](library-format.md#write-batching)): edits made during a frame, including those from the HTTP API and background imports since the last frame, are written together at the end of the frame, and a metadata backup follows when `mtime.json` or `metadata.json` changed. Pending writes are also flushed when a library is closed and, for every open library, when the app saves its state on quit. A failed write shows "Could not save".

Metadata backups have no setting: falcon backs up after a change the way Eagle does (Eagle has no preference for it either). An earlier "Back up metadata on change" checkbox that did nothing was removed; old saved state that has it still loads.

## Several libraries

Falcon can keep several libraries open in one window and switch between them; Eagle can't. Each open library is a workspace with all of its own state: where you are (scope, back and forward history, search, filters, sort and layout), the selection, the viewer, its undo and redo stacks, open dialogs and its image caches. Switching keeps all of it. Global state stays shared: Preferences, shortcuts, the recent list, zoom, panel visibility, the collect window and the HTTP API server.

- **Switching:** click the library name at the top of the sidebar, the ⇄ button in the sidebar header, or press ⌘L. The menu lists the open libraries first (name over its folder, dimmed, the shown one highlighted, × to close), then recent libraries that aren't open, then Open Library…, New Library…, Show in Finder, the collect window and Preferences. ⌘⇧[ and ⌘⇧] step through the open libraries.
- **Opening** a library that is already open shows it instead of loading it twice (paths are compared after resolving symlinks). A new workspace starts with the shown library's sort and layout and is added at the end of the list.
- **Closing** writes the library's pending changes, then picks the next library to the right (or the left at the end). Closing the last one leaves the empty window and stops the API.
- **Window title:** the shown library's name ("Falcon" when none is open). The title bar is hidden, so it shows in the Window menu, Mission Control and the Dock.
- **Launch:** the open libraries, and which one was shown, are saved with the app state (`libraries` and `active`) and reopened; missing ones are skipped. A library passed on the command line is opened (or shown, if it was already open) after them. State saved by older versions, which has only `library`, opens that one library.
- **HTTP API:** it serves the shown library and keeps its port when you switch. A `library/switch` request opens (or shows) that library in its own workspace and gives the one it replaced back its library; see [HTTP API](http-api.md#the-desktop-app).

What Eagle does (Eagle 4.0, from `js/directives/library-panel.js` and its template, `js/background.js`, `js/api-server.js` and `js/default-preferences.js`; observed unless marked):

- One library per window. The renderer keeps a single library path, and switching (`openLibrary`, then the `add-to-history-and-open` message to the background page) saves the cache, drops the current library and loads the new one in the same window. A dialog first asks to cancel running imports, downloads or saves. No code path opens a second main window for another library (inferred from the absence of one; the main-process window code isn't in the extracted renderer files).
- The library panel opens at the brand icon (`switchLibrary`) or with `library.switch`, ⌘L by default. It has a search field (fuzzy, with pinyin), then one row per entry of the `libraryHistory` setting: the library's icon (`icon.png` in the library, if any), its name without `.library`, its parent folder, and a check on the open one. Rows can be reordered by dragging, and their menu changes or removes the icon, shows the library in Finder, or removes it from the list. Below the list: Create, Load (open), Reload and Merge. ↑, ↓ and ↩ pick a row; Esc closes the panel.
- `libraryHistory` appends a newly opened library at the end, without duplicates, and keeps the user's order. A separate `pinnedLibrary` list exists.
- Item counts are not shown in the panel.
- `library/switch` over the API calls the same `openLibrary`, so the API always serves the window's library.
- Eagle has no next or previous library shortcut; `library.create` and `library.load` exist without defaults.

How falcon differs: several libraries open at once (above) and its own ⌘⇧[ and ⌘⇧] shortcuts; one window rather than one OS window per library (Eagle doesn't have several windows either); the recent list is newest first and capped at 10, with no search, icons, pinning, reordering, Reload or Merge yet.

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
| General | Add IPTC and XMP keywords as tags | `general.IPTC`, off by default | Same: every import from the app (drops, ⌘V, files passed to the app, the collect window) reads it (see [IPTC keywords](import.md)) |
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

## Organising

This covers folder icons, colours, sorts and passwords, drag reordering in the sidebar, nested smart folders, tag groups and saved filters. The code is split three ways:

- `crates/falcon-app/src/organize.rs` holds the decisions, with unit tests: drop zones, tree moves, which folders are hidden, sort keys, tag group sections and password form checks.
- `folder_icons.rs` holds the icon table.
- `app/organize_ui.rs` holds the egui side.

Library writes go through `falcon_library` (see Library format and Smart folders).

What Eagle does, from `app.bundle.js`, `index.html`, `js/directives/*.html` and `style/components/*.scss`:

| Question | Eagle | Seen or inferred | falcon |
|---|---|---|---|
| Folder menu | New Folder, New Subfolder, Move Folder, Quick Access, Rename, Copy as Link, Auto-Tag, a Password submenu (Create, Edit, Remove, Lock Now), a Sort By submenu that sorts the *subfolders* by title, expand and collapse entries, Duplicate, Export, Show Subfolder Content, Change Icon (submenu), a colour row, Remove. Subfolder, auto-tag, duplicate and export are disabled while the folder is locked | Seen | New Subfolder (disabled while locked), Rename, Quick Access, Move to Top Level, a Password submenu, a Sort Items By submenu, Change Icon, a colour row, Delete. There is no sort for subfolders by title |
| Item sort menu | Not in the single-folder menu. It is in the multi-folder menu and in the toolbar's layout panel. Entries: Global Settings (no own sort), Date Added, Date Modified, Date Created, Title, Extension, File Size, Dimensions, Rating, Duration, Random, then Ascending and Descending. The layout panel also lists Manual for folders | Seen | Same entries, offered as Sort Items By in the folder and smart folder menus and in the toolbar's layout menu. Tags is offered too, since falcon's sort supports it |
| Toolbar sort in a folder | While a folder or smart folder is open, the toolbar shows that folder's sort. Picking a key writes `orderBy` to the folder; the global sort changes only outside folders. Global Settings deletes `orderBy` and `sortIncrease`. The direction is written to the folder only when it has an `orderBy`; otherwise it changes the global direction | Seen | Same |
| Random and Manual | Random shuffles a cached order. A refresh button, a filter change and a folder reload clear the cache. Manual sorts by each item's `order[folderId]`, falling back to the import time. Dragging items within a folder switches it to Manual | Seen. That every visit reshuffles is inferred | Random shuffles once per visit to the folder. Duration sorts by length, with items that have none last. Manual keeps import order: falcon doesn't read the `order` map or let you reorder items by drag yet. Random and duration apply only when no search or colour filter is ranking the list |
| Icon picker | A Change Icon submenu holding a wrapping grid of 24 pt icons. The first, blank cell is the default icon | Seen | Same layout. Each of Eagle's 161 names gets the closest Phosphor Light glyph (`folder_icons.rs`), and `number10` to `number20` are drawn as digits. Eagle's artwork is not copied |
| Colours | A separate row in the menu: a "none" circle (which deletes `iconColor`), then red, orange, yellow, green, aqua, blue, purple and pink, 16 pt wide with 8 pt gaps. The colour tints the sidebar icon (not the label) and the folder's guide lines at 25%. Dark-theme values: `#FF6667`, `#FF9F0A`, `#FFD60A`, `#30D159`, `#40CBE0`, `#00AAFF`, `#C499FF`, `#FF99CC`; "none" is `#BDBEC0` | Seen | Same row and tints (`theme::ICON_*`). New subfolders take their parent's icon and colour (done in the library) |
| Reordering by drag | While a folder is dragged, each row shows three drop strips. A 9 pt strip at the top inserts before the row and shows a 1 pt primary-colour line. A 10 pt strip across the name nests the folder and shows a primary-tinted box with a 1 pt border and 6 pt corners. A 9 pt strip at the bottom inserts after. Dropping a folder into itself or a descendant is refused | Seen | The top and bottom quarters of a row show an insertion line and reorder; the middle half shows the box and nests (`organize::drop_zone`). `organize::tree_drop` works out the move and `Library::reorder_folder` makes it. Dropping on the Folders heading still moves a folder to the end of the top level |
| Lock icon | Folders with a password show a lock on their row: closed while locked, open once unlocked | Seen | Same, beside the count |
| Unlock screen | The content area shows an illustration, "Unlock to view Contents", "Hint: …" when a hint is set, and a password field. Enter checks the password, and a wrong one shakes the field. Touch ID and the licence code also unlock | Seen | Same without the illustration, Touch ID or licence code. A wrong password turns the field red |
| What a lock hides | Items in a locked folder, or in any folder below one, are left out of All, search and every other list. A locked folder's subfolders are hidden in the sidebar until it is unlocked | Seen | Same (`organize::hidden_folders`, `without_hidden`). An item filed in both a locked folder and another folder is hidden everywhere. Sidebar counts for All, Uncategorized and smart folders still include locked items. A locked folder's own count is hidden |
| How long an unlock lasts | Unlocking sets a runtime flag that is never saved, so a folder stays unlocked until Lock Now, a library reload or quitting. Leaving the folder doesn't re-lock it | Seen. That the flag isn't saved is inferred from the save code | Same: falcon keeps a set of unlocked ids for the session and clears it when a library opens. Creating or changing a password leaves the folder unlocked until Lock Now |
| Password dialog | New: Password, Enter again, Password Hints. Change: Old Password, New Password, Enter again, Password Hints. Remove: the current password (or the licence code) | Seen | Same fields (`organize::check_password_form`). Passwords with characters beyond Latin-1 are refused, because Eagle's `btoa` can't store them |
| Tag groups | The All Tags page has its own sidebar: All, Uncategorized, Starred, then "Groups (n)" with a + button and a drag-sortable list of groups. Each group row has a tag icon tinted with the group colour. The tag list is grouped A–Z by default; "Group by > Tag Group" shows a heading "Name (count)" per group with its tags under it, and an "Uncategorized" pseudo-group last. Each tag shows a dot in its group's colour. The + button creates "Untitled" and starts a rename. Dragging tags onto a group moves them there (with Alt, copies them). The group menu has Find items, Rename, Remove, Add to Library and a colour row. Removing a group keeps its tags, and asks first only if the group has tags. The main sidebar has no tag group section | Seen | The page has a Group by switch (Alphabet or Tag Group) and a New Group button. In Tag Group mode, each group gets a heading with its colour dot and count, followed by an "Ungrouped" section. Drag a tag onto a heading to move it into that group, or onto Ungrouped to take it out; the tag menu's Move to Group does the same. Double-click a heading, or use its menu, to rename it. The menu also recolours and deletes the group (keeping its tags, and confirming first when it has tags). Not yet built: the group sidebar, Alt-copy, descriptions and reordering groups |
| Saved filters | A Saved Filter button at the right of the filter bar opens a menu. "Save this filter" asks for a name; it is disabled when there is no filter and no search. The saved filters are listed below it, and clicking one applies it. Each has a more button with Rename and Remove (no confirmation). The menu can be sorted by drag and searched. Applying a saved filter replaces the whole filter and runs its saved search | Seen | A Saved menu in the filter bar holds Save This Filter… and the list. Clicking an entry applies it, replacing the filter and the search. Each entry's ⋯ menu has Rename, Move Up, Move Down and Remove. A notice names any saved rules falcon can't apply yet (dates, duration, camera, fonts, aspect ratios…) |
| Nested smart folders | The smart folder tree has carets, guide lines and the same three drop strips (nest or reorder). Smart folders take an icon and colour from the same picker and colour row. The dialog has no parent choice: the New Subfolder entry in a smart folder's menu passes the parent. The + menu also offers a smart folder group | Seen | The same tree, drag rules and icon and colour menu, with New Subfolder… in the menu. falcon's dialog also has an Inside choice (the parent), an icon and a colour, so it can move an existing smart folder too. Smart folder groups (folders with no rules) aren't built yet |

## Comments on a region

In the viewer, comment mode (the speech-bubble button or C) lets you drag a rectangle over the image. When you let go, a small box opens for the text: Save (or ⌘↩) adds the comment, Esc or Cancel drops it. Regions of 10 image pixels or less on either side are ignored, as in Eagle. Each comment is drawn as a numbered outline; hovering one shows its text. In comment mode, clicking an outline edits its text and its × button deletes it. Outside comment mode, a drag pans the image as usual. Grid cards show the number of comments in a blue disc at the top right. The code is in `comments.rs`.

What Eagle does, from the viewer's `comment-item` and `comments-container` directives, `js/annotation-preview.js` and the card template:

| Question | Eagle | Seen or inferred | falcon |
|---|---|---|---|
| Mode | A toolbar button with a count badge, shortcut C, hidden for video, audio, fonts, 3D models, web links, PDF and text | Seen | Same button, badge and key, hidden for the same kinds |
| New region | Drag a box; on release it is scaled to the original's pixels, rounded, kept only when both sides are over 10 px, and added at once with empty text; an editable box then opens on it | Seen | Same rule. The box is added when the text is saved, and is clipped to the image |
| Markers | Outlined boxes with a number badge and a × remove button; hovering shows the text in a floating box | Seen | Same |
| Moving and resizing | Dragging or resizing a box in comment mode updates `x`, `y`, `width` and `height`, which can then be fractional | Seen | Not built: regions keep the box they were drawn with |
| Text | Stored as HTML from a contenteditable box (`<br>` for line breaks) | Seen | Stored as plain text. Eagle's HTML is shown with the tags removed (`<br>` and block ends become line breaks, common entities decoded). Eagle will show falcon's line breaks collapsed and would read a `<` in falcon's text as HTML |
| Card badge | `has-annotation` card class: a 20 pt primary-blue disc with a white border at the top right, showing the count. Its title was meant to list the texts with tags stripped, but `replaceAll` is given the pattern as a string, so it shows the raw HTML | Seen | Same disc, without the title |
| Inspector | Comments listed with their texts; clicking one opens comment mode and scrolls to it | Seen | Listed with edit and delete buttons; clicking a region comment opens the viewer in comment mode |
| Undo | Deleting a comment offers a short-lived undo notice | Seen | Part of the normal undo history |

## Rotate and crop

The viewer bar has Rotate Left and Rotate Right for jpg, png, webp and bmp, and Crop for jpg, png and webp. Both rewrite the original (see [Crop and rotate](thumbnails.md#crop-and-rotate-observed)). The crop tool starts on the whole image. Drag its edges or corners, or drag inside it to move it; the size in pixels shows underneath. Save asks for confirmation, then overwrites the original. These edits can't be undone, so they clear the undo history. The rectangle math is in `crop.rs`.

## Folder covers

Eagle shows covers only on the subfolder cards at the top of a folder's content (`sub-folder-list` in `index.html`), not in the sidebar. From `calculateImageBinding` and `setFolderCover` (observed):

- The cover is the folder's `coverId` item, set with "Set as Folder Cover" (⌥⇧C) on a card while browsing a folder.
- Without one, it is the folder's first item in the current sort order (Eagle's default is newest import first). Trashed items, items with `noPreview`, text files and items in locked folders are skipped.
- A folder with neither takes the cover of its first subfolder that has one.
- Eagle writes no cover file. A crop clears the cached covers of the item's folders so they redraw.

falcon draws the subfolder cards in the Waterfall, Justified and Grid layouts with the same rule (`covers.rs`). Locked folders show a folder icon instead of a cover.

## Running it

```sh
cargo run -p falcon-app -- path/to/Some.library [ITEM…|FILE…]
scripts/bundle.sh   # builds target/release/Falcon.app
```

One item id (or an `eagle://item/<id>` link) opens that item in the viewer. Several ids select them. Files are imported.
