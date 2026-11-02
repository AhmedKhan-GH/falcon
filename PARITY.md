# Parity with Eagle

This is the checklist of everything Eagle 4.0.0 can do, compared with falcon. It was built on 2026-11-01 from Eagle's shipped app code and its English UI text, and every falcon status was checked in falcon's code, not taken from ROADMAP or the docs. Local parity means every row marked local is Done. Rows marked online or plugin need a network service, an account or Eagle's plugin system, and don't count.

Statuses: **Done** works as in Eagle. **Partial** exists but is missing options or behaves differently. **Missing** isn't there. **Not wanted (user decision)** is an Eagle feature the user chose to leave out; it is not counted as Missing.

| Area | Done | Partial | Missing (local) | Missing (online/plugin) |
|---|---|---|---|---|
| Main window: menus, shortcuts, toolbar, sidebar, layouts | 258 | 14 | 1 | 8 |
| Viewers and file formats | 193 | 15 | 1 | 5 |
| Search, filters and organising | 215 | 5 | 0 | 3 |
| Import, export, library, preferences and integrations | 189 | 11 | 7 | 27 |
| **Total** | 855 | 45 | 9 | 43 |

Rows marked **Not wanted (user decision)** (4) are Eagle features the user chose not to have in falcon; they are counted outside the local Missing total.
## Fix first

These are bugs, not missing features. The first four can lose data or edits.

1. ~~Dropping a `.library` folder on the window imports every file inside it as new items.~~ Fixed: it opens the library.
2. ~~Editing a smart folder keeps only one condition and drops NOT flags and rule types the dialog doesn't offer.~~ Fixed: every condition is edited and unknown rules are kept.
3. ~~The folder name and description in the inspector are never saved.~~ Fixed.
4. ~~Clicking a folder chip in the item inspector removes the item from that folder.~~ Fixed: it opens the folder, and the chip's × removes.
5. ~~Smart folder rules on comments, camera, ISO, aperture, focal length, shutter, duration, timestamp and font never match; `regex` is only a substring test; `on` compares UTC days.~~ Fixed.
6. ~~Saved filters drop date, duration, camera and font rules when saved.~~ Fixed; they are saved and restored in Eagle's format.
7. ~~The app opens libraries without the backup fallback.~~ Fixed: the app restores from the newest good backup and backs up on change and every 30 minutes.
8. ~~Keys that differ from Eagle: ⌥1 and ⌥3 are swapped, Back and Forward, Rename. Dragging items onto a folder adds them.~~ Fixed: ⌥1 Waterfall / ⌥3 Grid, ⌫ and ⌘←/⌘→ for history, ⌘R renames, ⌘[ ⌘] move folders, and a drop onto a folder moves unless ⌥ is held.

## Biggest gaps

1. ~~Every viewer opens inside the window between the sidebar and inspector in Eagle, with a toolbar button that hides both. Falcon does this only for PDFs.~~ Fixed: every viewer draws in the central panel, with the panels button, ⌥⌘3 and Tab.
2. ~~No application menu bar.~~ Fixed: native File / Edit / Find / Organize / View / Window / Help menus with live shortcuts and falcon's own Actions menu while falcon has actions (Eagle's is the `ai-action` plugin).
3. ~~Preferences: 8 panes and about 30 settings against Eagle's 13 panes and about 80 settings; the rest are listed in §6.~~ Fixed: Preferences has every local pane and setting of Eagle's except GPU acceleration (not built); vibrancy, the screenshot notice, the App menu's Color Space and the sound effects are Partial, and plugin shortcuts and plugin developer tools are open with the plugin system. The extension, Proxy, AI Search, AI Models and MCP settings are online or plugin and absent.
4. ~~The full-size image is capped at 3072 px.~~ Fixed: the original is tiled at full resolution; HEIC/AVIF/JXL still come from a 3072 px QuickLook render (no decoders).
5. ~~Folder import flattens structure; no duplicate check on import; no import error list.~~ Fixed.
6. ~~No export of items or folders, and no CSV export.~~ Fixed.
7. ~~Shortcuts still missing: T, F, ⌘J Go to Folder, ⇧+arrows, ⌘D, ⌘T and the tag clipboard, ⇧D, ⇧T, F5, ⌘O and more (see §3).~~ Fixed: the shortcut table has every local macOS Eagle id; the 18 without a row are Windows variants, online importers, reverse image search and plugin services (see the Shortcuts row).
8. ~~No marquee selection, no multi-select of folders, no grid display options.~~ Fixed.
9. ~~No tag picker or suggestions.~~ Fixed.
10. ~~No GIF player controls, no animated WebP or AVIF, no GIF hover previews.~~ Fixed (AVIF on macOS); hover playback for video and audio is in.
11. ~~No interactive 3D viewer, text editor or full font viewer.~~ Fixed for obj/stl/ply/glb/3mf/fbx/dae/3ds and ifc with full geometry through web-ifc; only an ifc file with no drawable geometry stays a still.
12. ~~No watched import folder, no reload on outside changes, no sync-conflict detection, no screen capture.~~ Fixed.
13. ~~English only; Eagle ships 8 languages.~~ Fixed: the whole interface in all eight (machine-written translations, pending a native read).

The full tables follow. Each section ends with that area's own ranked gap list.

## Main window: menus, shortcuts, toolbar, sidebar, layouts


Scope: application menus, context menus, keyboard shortcuts, toolbar, sidebar, grid layouts, selection, drag and drop, window behaviours, themes, status UI, onboarding and empty states.

Paths: Eagle paths are relative to `eagle-src/app/` (`bundle` = `app/app.bundle.js`, `prefs` = `app/js/default-preferences.js`, `index` = `app/index.html`, `layout-panel` = `app/js/directives/layout-panel.html`, `i18n` = `i18n/en.js`). Falcon paths are relative to `crates/falcon-app/src/`.

Every falcon status here was checked in the code. ROADMAP and docs were not used. Rows that belong to other audits (viewers, search and filters, import, export, library, preferences, API) appear only as cross-references at the end of each section and are **not counted**.

Overall finding: falcon now has a native menu bar (`crates/falcon-app/src/menu.rs`, AppKit through `objc2-app-kit`) with File, Edit, Find, Organize, View, Window and Help menus whose items show their live shortcuts, enable and disable with the app's state and carry checkmarks. It holds only commands falcon has, plus falcon's own Actions menu while falcon has actions (Eagle's is the `ai-action` plugin, and falcon keeps its actions in its app data, never in `actions.config.json`); Eagle's online items are absent.

---

### 1. Application menubar

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| A menubar with File / Edit / Find / Organize / View / Actions / Window / Help menus, each item showing its live shortcut | Partial | Native AppKit menus in `crates/falcon-app/src/menu.rs` (File, Edit, Find, Organize, View, Window, Help) with live shortcuts, enabled rules and checkmarks; clicks run through `App::run_command`. Actions is falcon's own (Eagle's is the `ai-action` plugin), shown while falcon has actions; only commands falcon has | local |
| App menu: About | Done | `ui/dialogs/about.rs`, replacing winit's stock item | local |
| App menu: Preferences… (⌘,) | Done | shortcuts.rs `app.preferences`; in the app menu after About (`menu.rs`) and in the library switcher | local |
| App menu: Lock Now (⌘⌥L, locks the whole app behind a password or Touch ID) | Done | ⌥⌘L locks the app (`applock.rs`); unlock by password or Touch ID (`touch_id.rs`, LAContext through `msg_send!`, pref `touch_id` shown only when available) | local |
| App menu: Color Space (sRGB / Display P3 / Unmanaged) | Partial | App menu ▸ Color Space after Preferences with Eagle's three choices, order and default (Unmanaged), persisted in prefs and applied to the window's `CAMetalLayer.colorspace` (found among the view's sublayers since 700506ff; before that the menu did nothing) at start and on change (`colorspace.rs`; re-applied each frame because wgpu resets it on resize), disabled while locked or Preferences is open. Gap: egui's pixels are sRGB content, so the choice changes how the display interprets the window; there is no per-image colour management, and the visible effect is still to be checked on screen | local |
| App menu: Hide, Hide Others, Quit | Done | Supplied by winit's default macOS app menu (main.rs uses eframe defaults) | local |
| App menu: Check for Updates, Register / Unregister, Manage devices | Missing | Not applicable to falcon | online/plugin |
| File: New… (⌘N), a searchable menu of folder, smart folder, imports, duplicates and new documents from templates (Sketch, PSD, AI, XD, TXT, Word, PPT, Excel, Keynote, Pages, Numbers, MindNode, XMind) | Partial | File ▸ New…, ⌘N (`file.create.new`) and the sidebar +: type-to-filter menu with New Folder, New Subfolder, New Smart Folder…, Import Folders / Links / Files / Eagle Package… (Import Links…: paste or drop URLs to make `.url` items, titles fetched for the user's own URLs only, `ui/dialogs/import_links.rs`; a missing, octet-stream or wrong content type is checked against the first bytes, `link_import::sniff`, so an image is still downloaded; a bookmark's thumbnail is a screenshot of its page at Eagle's 1440x900 after 2.5 s: `page_shot/` queue on the UI thread, one page at a time in an off-screen WKWebView with a non-persistent store, third-party loads and cookies blocked, http/https only, no dialogs or new windows, 20 s load timeout then the plain icon with no retry; also through Refresh Thumbnail for existing `.url` items; a `.url` dropped in or auto-imported waits for Refresh, unlike Eagle. Tested against real WebKit on a local page (`tests/page_shot.rs`); Partial until seen on screen), Find Duplicates ▸ All Items / Current List / Selected (disabled when the list is empty or fewer than two items are selected), Auto-Import ▸ Settings… (opens the Auto-Import pane, with the watched folder shown while on) and Text Document; Photoshop, Illustrator, Word, PowerPoint, Excel and XMind documents generated by falcon (`falcon_library::templates`: `Untitled.psd` (white 1920 × 1080 RGB 8-bit, RLE, from the public PSD spec) / `.ai` (one-page 1366 × 768 PDF-compatible) / `.docx` / `.pptx` / `.xlsx` / `.xmind`, imported into the current folder and selected). Tests: falcon's own PSD reader decodes the PSD as white, `sips` reads it, the AI's xref offsets are checked and `sips` reads it as a one-page PDF; the PSD and AI were not opened in Photoshop or Illustrator, and the menu entries are not yet seen on screen. Sketch, XD, Keynote, Pages, Numbers and MindNode listed disabled: proprietary formats falcon can't write correctly yet; Eagle's exact placement of the entries is in compiled code and unchecked; Pinterest and ArtStation are online | local |
| File: New Folder (⇧⌘N) | Done | A sibling right after the open folder, or top level when none is open, as Eagle's `newFolder` (`Action::CreateSibling`) | local |
| File: New Subfolder (⌥N) | Done | ⌥N, File menu and folder menu; inserted first among its siblings, as Eagle | local |
| File: New Smart Folder (⇧⌥⌘N) | Done | shortcuts.rs; opens the smart folder dialog | local |
| Edit: Undo / Redo | Done | ⌘Z and ⇧⌘Z with a capped transaction history; Edit menu entries go to a focused text field first | local |
| Edit: Cut / Copy / Paste / Select All | Done | Menu entries; Cut is the text role only, as in Eagle; ⌘C copies files, ⌘V imports, ⌘A selects all | local |
| Edit: Rename (⌘R on macOS) | Done | ⌘R (F2 alias): one item renames through the inspector title; several open the Rename N Items dialog with Eagle's templates (`rename.rs`) | local |
| Edit: Copy File Path (⌥⌘C) | Done | Key and item menu copy every selected path, one per line (`ui/dialogs/export.rs` `copy_variant`) | local |
| Edit: Copy Folder Path (⌥⇧⌘C), Copy Eagle Link, Copy Thumbnail, Copy Base64, Copy Name | Done | Edit ▸ Copy Details: Folder Paths, Eagle Links (`http://localhost:<port>/item?id=`, as Eagle's Copy Eagle Link), Thumbnail, Base64, Names; all for every selected item | local |
| Edit: Duplicate item (⌘D) | Done | The first selected item, as Eagle's `duplicateItem`; the card entry is single-item only, as Eagle | local |
| Edit: Refresh Thumbnail (⌥⌘R), Custom thumbnail from a file (⌥⌘T), Reset custom thumbnail | Done | ⌥⌘R, ⌥⌘T, Edit ▸ Custom Thumbnail ▸ From File / From Clipboard / Reset (`edit.thumbnail.custom.reset`), card menu; Reset on the first item, as Eagle | local |
| Edit: Custom thumbnail from the clipboard (⇧⌥⌘T) | Done | Clipboard image or copied file | local |
| Edit ▸ Move Folder: Up, Down, To Top, To Bottom (⌘] ⌘[ ⇧⌘] ⇧⌘[) | Done | Keys and Edit menu entries move the open folder or smart folder among its siblings, undoably (`organize::arrange`) | local |
| Edit ▸ Edit Image: Horizontal Flip (⇧F), Rotate (⇧R), Crop (⇧C), Combine Images (⇧⌘M) | Partial | Flip, Rotate and Crop (⇧C, `edit.image.crop`) in the menu and keys; Combine Images is a plugin in Eagle 4, not built | local |
| Edit ▸ Edit Folder: Set Current Folder (⇧⌘R), Move Folder, Password create / edit / remove / lock (⇧⌘L) | Done | ⇧⌘R Folder Settings, Edit ▸ Move Folder…, Edit ▸ Folder Password ▸ Create / Change / Remove / Lock Now (⇧⌘L), with Eagle's ids and enabled states | local |
| Edit: Remove from current folder (⇧⌘⌫) | Done | shortcuts.rs `edit.remove.folder`, menu and card menu; one undo step, no prompt (Eagle's `removeFromFolder` asks nothing) | local |
| Edit: Move to Trash (⌘⌫) | Done | shortcuts.rs:114, app/commands/mod.rs `run_command` | local |
| Find: Go to Folder (⌘J or J), a quick-switch dialog with Folders / Smart Folders / Tags tabs | Done | `find.quicksearch` ⌘J and bare J: Folders / Tags / Smart Folders / Items tabs, ranked by `string_score`, recents (200 kept) | local |
| Find: Add to Folders… (⇧⌘J, a searchable folder picker that can create folders) | Partial | `ui/dialogs/add_to_folders.rs`, `folder_picker.rs`: Find menu, ⇧⌘J, item menu and the inspector's + (one or several items); All / Recent / Selected tabs, search, Create “…”, tree with keys; no Save: closing (click outside, Esc) applies the ticks as one undo step, nothing if unchanged; the “Take them out of the open folder” box stays below the list when a folder is open (Eagle's Add to Folder dialog has it, its panel doesn't). Rules tested; not yet seen on screen. Gaps: Eagle's ⇧⌘J and Find menu open its older Save / Cancel dialog, and only F, Organize ▸ Folder ▸ Add and the inspector's + open the tabbed panel, while falcon uses the panel for all of them; the panel is placed at the pointer but not moved clear of the inspector; search is substring, not Eagle's fuzzy / pinyin match | local |
| Organize: Add To Last Used Folder (⇧D) | Done | `organize.folder.addLast`: 50 recent folders kept app-wide, as Eagle's localStorage list | local |
| Organize: Rate 0–5 stars from the menu and keys | Done | Organize menu and the 0–5 keys (`organize.rating.*`) with a toast; in the viewer the bare digits keep their zoom meaning | local |
| Organize: Add Tags… (⌘T), Copy Tags (⇧⌘C), Paste Tags (⇧⌘V), Clear Tags | Done | Organize menu; shortcuts.rs `organize.tag.*` (Clear Tags unbound by default, as Eagle) | local |
| View: Always on Top (⇧T) | Done | `modes.rs` `toggle_on_top` through `ViewportCommand::WindowLevel`; View menu checkmark; off at launch as Eagle | local |
| View: jump to All / Uncategorized / Untagged / Recent / Random / All Tags / Trash (⌘1–⌘7) | Done | shortcuts.rs `view.all` … in Eagle's View menu order; from the viewer they close it first | local |
| View: layout Waterfall / Justified / Grid / List with checkmarks (⌥1–⌥4) | Done | ⌥1–⌥4 in Eagle's order; View menu entries with checkmarks (`menu.rs` `validateMenuItem`) | local |
| View: Scroll to Top / Bottom / Page Up / Page Down (Home, End, PgUp, PgDn) | Done | Home, End, Page Up and Page Down scroll the grid (a page is the view less 72 px, as Eagle); ⌘↑ / ⌘↓ select the first or last item | local |
| View: Zoom In / Out (⌘= / ⌘-) | Done | Thumbnail size (shortcuts.rs:96-97, app/commands/mod.rs `run_command`) | local |
| View: Actual Size (⌘0) / Zoom to Fit (⌘9) | Done | View ▸ Actual Size ⌘0 and Zoom to Fit ⌘9 (`view.zoom.actual`, `view.zoom.fit`), enabled in the viewer | local |
| View: Grayscale preview (⌥⌘G) | Done | Every loader tier incl. grid cards, animations, video frames, PDF pages and font specimen (`logic::grayscale_rgba`), for the main window's grid and viewer only, as in Eagle; a preview window has its own flag (see the Navigator row in the viewers section); custom thumbnails gray too, unlike Eagle | local |
| View: Open in New Window (⌘O) | Done | Every acted-on item except audio in a new window per open, stepped with ←/→ (`ui/viewer/preview_window/mod.rs` `open_preview_items`), as Eagle's one window that steps | local |
| View: Open With Default App (⇧↩), Open with Other App, Reveal in Finder (⌘↩) | Partial | ui/viewer/open.rs `open_keys`, shortcuts.rs:104-105; View ▸ Open With Another App… asks for an app (Eagle's `openWithOther` sends `open-with-dialog`, in its compiled main process) and opens the first selected item with it; `view.file.openother` is an unbound row. Not yet seen on screen | local |
| View: Open Link in Browser (⇧⌘O) | Done | `view.file.openlink`, `urls.rs` (web URL or file path) | local |
| View: show or hide Sidebar (⌥⌘1), Inspector (⌥⌘2), both (⌥⌘3) | Done | shortcuts.rs `view.toggle.sidebar`, `view.toggle.inspector`, `view.toggle.all`; Tab also toggles both; all work in the viewer too | local |
| View: Show Name (⌥⌘4), Show Item Info (⌥⌘5), Show Comment count (⌥⌘6) | Done | shortcuts.rs `view.toggle.listname/listmetas/listannotation`, View menu checkmarks, `logic::Display` | local |
| View: Show Subfolder Content (⌥⌘7) | Done | `view.toggle.subfolder`, `logic::folder_members`, `grid_order` in ui/organize.rs; the layout panel and the strip heading have the switch too | local |
| View: Enter / Leave Presentation Mode (F5) | Done | `view.toggle.slideshow`: full screen with panels and the viewer bar hidden; arrows step, Esc leaves. Eagle has no timer or shuffle either | local |
| Actions menu: user-defined Actions, each with its own shortcut | Done | Falcon's own implementation of a feature that is a plugin in Eagle (`ai-action`): a native Actions menu between View and Window, shown only while falcon has actions for the open library, each entry with its accelerator and enabled state (`action_enabled`: selection, not Trash, runnable); panel with search, run, Edit/Duplicate/Remove and New Action; editor with steps, tag and folder pickers and a shortcut recorder that refuses conflicts (`crates/falcon-app/src/actions/`). Actions are stored in `<app data>/actions/<path-hash>.json` (atomic writes); falcon never reads, creates, changes or deletes `<library>/actions.config.json` (`actions/storage_tests.rs`) | local |
| Window: Toggle Full Screen, Minimize, Close Window (⌘W) | Done | ⌃⌘F, ⌘M and ⌘W through eframe viewport commands; ⌘W closes the viewer first, then the window (which quits falcon, like the red button) | local |
| Help: Help Center, Show Tutorial, Install Extensions, Feedback, Recover license, Change Logs | Missing | Not applicable; Help ▸ Show Tips is falcon's local overlay (see Tutorial / tips overlay) | online/plugin |
| Help: Error Logs, Export Debug Report… | Done | Errors and panics logged to `~/Library/Logs/Falcon`; Help ▸ Error Logs and Export Debug Report (a zip of system, library and log details; no network or disk tests) | local |

Cross-references (not counted): File ▸ Import Folders / Links / Eagle Package / Pinterest / ArtStation / Auto Import, File ▸ Export to Computer / .eaglepack / Specified Format / CSV, Library menu (Create / Switch / Merge / Clear History / Empty cache and reload) and Find ▸ Search / Search All / Filter submenu / Search by Image / Find Duplicates belong to the import, export, library and search audits. Falcon's library switcher popup (`ui/sidebar/mod.rs`) covers Open, Recent, New, Show in Finder, Collect Window and Preferences.

---

### 2. Context menus

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Every context menu has a type-to-filter search box with keyword matching | Partial | `ui/menus/search.rs` on the item, folder, smart folder, tag, tag group, palette, New, filter + and saved-filter menus incl. submenus, Change Icon (searched by icon name), Add/Move to Group and the rename Insert / Number / Date menus (`ui/menus/insert.rs`); the macOS Help menu searches itself. Missing: Eagle's inspector Help button menu (falcon has no such button; its entries are mostly online links and the local ones are in the app menu) | local |
| **Item**: Restore (in the Trash) | Done | ui/menus/item.rs `item_menu_body`, which also offers Delete Permanently | local |
| Item: Open Item(s) in New Window | Done | Card menu opens all its targets in one window stepping with ←/→; middle click opens its card | local |
| Item: Open With Default App / Open With ▸ apps / Reveal in Finder | Done | ui/viewer/open.rs `open_menu` | local |
| Item: Open File Location ▸ All or one of the item's folders (jumps to the item there) | Done | Card menu; jumps to the item in that view | local |
| Item: Plugin ▸ recent plugins | Missing |  | online/plugin |
| Item: Add To Last Used Folder | Done | Card menu, ⇧D | local |
| Item: Add to Folders… (picker panel) | Done | Opens the Add to Folders picker | local |
| Item: Share (macOS share sheet) | Done | `share.rs` through `NSSharingServicePicker` at runtime | local |
| Item: Pin to Top / Unpin from Top inside a folder | Done | `item_ops.rs`: item key `pinned: {folderId: ms}`; pinned first in that folder; badge on cards and list rows | local |
| Item: Set as Folder Cover | Done | image_edit.rs:90-95 | local |
| Item: New Folder with Selection (N items) | Done | `folders_ui` `new_with_dialog`; the new folder becomes the last-used folder for ⇧D | local |
| Item: Rename / Rename N Items (batch rename dialog) | Done | Rename N Items dialog (`rename.rs`, incl. `%F`), asks first at 10+ items; one item uses the inspector title | local |
| Item: Copy (files) | Done | Edit ▸ Copy, the item menu's Copy and ⌘C copy the selected files (`ui/dialogs/export.rs` `copy_items`) | local |
| Item: Copy File Path | Done | `ui/dialogs/export.rs` `copy_variant`, every selected item | local |
| Item: Copy… ▸ Link, Folder path, Thumbnail, Base64, Name | Done | `ui/dialogs/export.rs` `item_copy_menu`; Link copies the item's source URL | local |
| Item: Copy Tags / Paste Tags | Done | `item_menu` in ui/menus/item.rs | local |
| Item: Combine Images (several selected) | Missing | An Eagle plugin (`combine-images`), not app functionality | local |
| Item: Duplicate | Done | Card menu for one item, ⌘D, Edit menu; copies the first selected item exactly as Eagle's `duplicateItem` does | local |
| Item: Search by Image ▸ Google / Bing / Yandex / TinEye / SauceNAO | Missing |  | online/plugin |
| Item: Enter / Leave Presentation Mode | Done | Item menu and middle click in the image viewer | local |
| Item: Thumbnail Background ▸ None / Black / Gray / White / Checkerboard grid | Done | Item menu submenu for the selection (`logic::ThumbBg`, `Action::Background`); hidden in the viewer as Eagle | local |
| Item: Grayscale preview toggle | Done | Item menu | local |
| Item: More ▸ custom thumbnail (file) / reset | Done | image_edit.rs:96-103 | local |
| Item: More ▸ custom thumbnail from the clipboard | Done | ⇧⌥⌘T and the card menu | local |
| Item: More ▸ Refresh Thumbnail | Done | image_edit.rs:104 | local |
| Item: More ▸ Re-analyze colors | Done | Labelled "Refresh Palette" (image_edit.rs:107) | local |
| Item: More ▸ Replace with File… | Done | Confirmation, copy in, swap through a `.bk` that is deleted at once (`item_ops.rs` `swap_in`), thumbnail redrawn | local |
| Item: Remove from this Folder | Done | Card menu entry and ⇧⌘⌫ | local |
| Item: Move to Trash | Done | ui/menus/item.rs `item_menu_body` | local |
| Item (falcon only): Rotate Left / Right, Flip H / V, Crop… | Done | image_edit.rs:110-132 (falcon extra) | local |
| **Folder**: New Folder (sibling) | Done | `Action::CreateSibling`, right after the clicked folder under the same parent | local |
| Folder: New Subfolder | Done | ui/sidebar/mod.rs `folder_row` | local |
| Folder: Move Folder… (picker dialog) | Done | `folders_ui` `move_dialog` with destination search; `Library::move_folders` (above, into or below a target) | local |
| Folder: Add to / Remove from Quick Access | Done | ui/sidebar/mod.rs `folder_row` | local |
| Folder: Rename | Done | Inline rename (ui/sidebar/mod.rs `folder_row`, ui/sidebar/folders.rs `finish_rename`) | local |
| Folder: Copy Link (eagle:// folder link) | Done | `http://localhost:<port>/folder?id=` as Eagle; `eagle://folder/<id>` under Copy ▸ Eagle Link; smart folders likewise | local |
| Folder: Set Auto-Tag | Done | "Auto-Tags…" in the folder menu and ⇧⌘R (`ui/organize.rs` auto_tag_dialog) | local |
| Folder: Password ▸ Create / Edit / Remove / Lock Now | Done | ui/organize.rs `folder_password_menu` | local |
| Folder: Sort By ▸ Title A-Z / Z-A, current level or all levels | Done | ui/organize.rs `folder_title_sort_menu` (falcon asks to confirm, like Eagle) | local |
| Folder: Expand/Collapse ▸ this folder, same level, all folders | Done | Expand/collapse entries in the folder menu; ⌘/⌥/⌘⌥ clicks on carets | local |
| Folder: Duplicate | Done | `Library::duplicate_folder`: copy placed after the original, new ids, no password, no items | local |
| Folder: Show Subfolder Content | Done | Folder context menu entry runs the same command as View ▸ Show Subfolder Content, ⌥⌘7 and the layout panel | local |
| Folder: Change Icon… and colour row | Done | ui/organize.rs `folder_menu_extras` | local |
| Folder: item sort for this folder (Order By) | Done | "Sort Items By" (ui/organize.rs `folder_menu_extras`) | local |
| Folder: Remove Folder | Done | Asks only when the folder has items or subfolders, with a ticked "trash the items too" box that can be unticked; disabled while locked | local |
| **Several folders selected**: menu with batch rename, shared order, move, quick access, export, icon, colour, remove | Done | `multi_folder_menu` in `folders_ui`: batch rename (Eagle's templates, `rename.rs`), shared sort, Move Folder…, Quick Access, Export to Computer, icon and colour for all, remove. Falcon's icon grid instead of emoji | local |
| **Smart folder**: New Smart Folder (sibling) / New Subfolder / Rename / Edit Rules / Quick Access / Change Icon / Remove | Done | ui/organize.rs `smart_row` | local |
| Smart folder: Duplicate | Done | `smart::duplicate_smart_folder`: deep copy placed before the original, `parent` keys fixed | local |
| Smart folder: Refresh | Partial | Falcon addition (Eagle 4's smart-folder menu has no Refresh entry): non-group smart folder menu; flushes, re-reads the library and shows a toast with the fresh count (`refresh_smart_folder`). Not yet seen on screen | local |
| **Smart Folders heading +**: New Smart Folder / New Group | Done | ui/organize.rs `smart_section` | local |
| **Quick Access item**: Remove from Quick Access | Done | ui/sidebar/mod.rs `sidebar` | local |
| **Subfolder card** (strip above a folder's items): Rename / Auto-Tag / Password / Export / Remove | Done | Card menu: Rename, Auto-Tags…, Password ▸, Export ▸, Delete Folder (⌘⌫); Rename/Auto-Tags/Delete disabled and Export hidden while locked, as Eagle | local |
| **Trash** sidebar item: Empty Trash | Done | ui/sidebar/mod.rs `nav_rows` | local |
| Trash sidebar item: Restore All Items | Done | Trash menu, after asking | local |
| **Recently Used** item: Clear history | Done | Recently Used menu | local |
| **Sidebar nav area**: choose which entries are visible (All, Uncategorized, Untagged, Recent, Random, Community, All Tags) | Done | Right-click on All / Untagged / Random ticks entries on and off; Preferences ▸ Sidebar has the same switches; Community has no counterpart | local |
| **Sidebar / panel edges**: Toggle Sidebar / Inspector / both | Done | View menu, ⌥⌘1/2/3, the toolbar panels button and its right-click menu | local |
| **Folders heading**: Sort folders by title | Done | ui/sidebar/mod.rs `sidebar`, ui/organize.rs `folder_title_sort_menu` | local |
| **Empty grid** right-click opens the layout and sort panel | Done | Right-click on empty grid space opens the layout panel in all four layouts and the empty view; the selection is kept | local |
| **List-view column header** right-click picks columns (tags, dimensions, rating, extension, file size, date added) | Done | `layout_panel::list_header` | local |
| **Toolbar title** right-click lists the ancestor folders to jump to | Done | Right-click lists the folder (checked) and its ancestors, hover shows the path; single subfolders only, as Eagle | local |
| **Tag** (All Tags page): Rename / Star / Move to Group / Remove from Group / Remove | Done | ui/menus/tags.rs `tags_menu`, ui/organize.rs `tag_group_menu` | local |
| Tag: Filter items with the tag(s), Merge N tags (several selected), Add to several groups | Done | `tags_menu` in ui/menus/tags.rs, `merge_dialog` (2–5 tags as Eagle), `Library::merge_tags`, `add_tags_to_group_too` | local |
| **Tag group**: Rename / Remove | Done | ui/organize.rs `tag_group_context_menu`, which adds description, move and colour | local |
| Tag group: Find items with any tag in the group | Done | Group heading menu, `filter_any_tags` | local |
| **Tag list sort**: A>Z / Z>A / by count / group by Alphabet or Groups | Done | Sort and group-by saved app-wide, as Eagle's localStorage keys | local |
| **Palette swatch** (inspector): Find similar colour, copy as HEX / RGB / RGBA / HSL / HSV / HWB / CMYK, set as primary | Done | Find Similar, copy as HEX/RGB/RGBA/HSL/HSV/HWB/CMYK (`colors.rs`), Primary Colour toggle with a ring on the primary swatch | local |
| **Inspector tag input**: Copy / Paste / Clear All Tags | Done | `tag_area_menu` in `app/tag_picker.rs` | local |

---

### 3. Keyboard shortcuts

Eagle defaults come from prefs `shortcuts.keybinds` (rebindable) and from the hard-coded Mousetrap table at bundle 49291-49340.

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Shortcut editor: rebind, conflict check, clear, reset, grouped list | Done | shortcuts.rs:150-200, 400-530 | local |
| Shortcut editor: search field ("Search Shortcut") | Done | Search field over name, group, id and chord in Preferences ▸ Shortcuts | local |
| Shortcut editor coverage: the roughly 150 rebindable commands | Done | Every falcon command listed under Eagle's id with Eagle's default (`.darwin` ids with migration of saved overrides); Eagle ids with no falcon action are listed in `docs/src/app.md` | local |
| ⌘F search, ⇧⌘F toggle filter | Done | shortcuts.rs:108-109 | local |
| ⌥⌘F search all, ⌥⇧F filter sidebar, ⇧⌥⌘F reset filter, ⌥F / ⌥T / ⌥C / ⌥S / ⌥R / ⌥D / ⌥E open one filter | Done | `find.search.all`, `find.sidebar.filter`, `find.filter.reset`, `find.filter.*` | local |
| ⌘L switch library | Done | shortcuts.rs:117, app/commands/mod.rs `run_command` | local |
| ⌘E export, ⇧⌘E export .eaglepack, ⇧E export as format | Done | shortcuts.rs Export group | local |
| ⌘1–⌘7 sidebar views | Done | shortcuts.rs `view.all`, `view.unfiled`, … | local |
| ⌥1–⌥4 layouts | Done | shortcuts.rs `view.layout.*`, in Eagle's order | local |
| ⌥⌘1–⌥⌘8 panel and caption toggles plus Navigator | Done | ⌥⌘1–⌥⌘7 and ⌥⌘8 Navigator (`view.toggle.navigator`) | local |
| ⇧T always on top, F5 slideshow, ⌥⌘G grayscale | Done | shortcuts.rs `view.alwaysOnTop`, `view.toggle.slideshow`, `view.grayscale` | local |
| ⌘O open in new window, ⇧⌘O open link | Done | ⌘O opens the acted-on items in a new window; ⇧⌘O opens the link | local |
| ⇧↩ default app, ⌘↩ reveal | Done | shortcuts.rs:104-105 | local |
| ⌥⌘S reveal, ⌥⌘D duplicate (view) | Done | `view.reveal.darwin` ⌥⌘S and `view.duplicate.darwin` ⌥⌘D (inert in Eagle itself; macOS reserves ⌥⌘D for the Dock) | local |
| Home / End / PageUp / PageDown scroll the grid; ⌘↑ / ⌘↓ jump to top or bottom | Done | `run_command` scroll offsets; ⌘↑ / ⌘↓ also select the first or last item, where Eagle only scrolls | local |
| 0–5 set the rating of the selection | Done | `organize.rating.*` in the grid and every viewer (bare 0/1 no longer zoom the image viewer, as Eagle); the web viewer takes the keys while its page has focus | local |
| T opens the tag panel, F opens the folder panel, J opens Go to Folder, G opens Actions, P opens Plugins | Done | T opens the tag picker, F the Add to Folders picker (`organize.folder.panel`), J Go to Folder; G opens falcon's Actions panel (`organize.actions.panel`; Eagle's is the `ai-action` plugin); P (Plugins) is not applicable | local |
| R reshuffles the Random view | Done | R and View ▸ Shuffle Again; also a folder or smart folder sorted Random | local |
| `*` or `/` expand or collapse every folder | Done | Eagle's `toggleAllFolders` rule (smart folders when one is open); `*` is the ⇧8 alias chord | local |
| Tab toggles all panels | Done | Fixed key in shortcuts.rs FIXED, when no text field has focus (`logic::toggle_all`) | local |
| Space opens the quick preview (preference: preview, native Quick Look, or page down) | Done | `Prefs::space`: previews the first selected item (any selection), Quick Look, or page down (no selection needed) | local |
| ⇧Space pages up | Done | Fixed key | local |
| Enter toggles the viewer | Done | Enter is an alias of Viewer (shortcuts.rs:95) | local |
| Esc closes panel, slideshow or viewer, otherwise clears the selection | Done | app/commands/mod.rs `keyboard` | local |
| Arrow keys move the selection; ←→ step in the viewer | Done | app/commands/mod.rs `keyboard`, `viewer_step` | local |
| WASD as arrow keys | Done | Fixed keys | local |
| ⇧ + arrows extend the selection | Done | `logic::extend_selection`: ⇧←/→ grow or shrink from the anchor; ⇧↑/↓ only in List layout, as Eagle | local |
| Arrow keys move through the sidebar tree when it has focus (focus zones: content / sidebar / tags) | Done | Content, sidebar and tags zones: → from the sidebar enters the tags zone on All Tags, ← leaves, ↑/↓ walk All → Uncategorized → Starred → groups, ⌘⌫ deletes the open group | local |
| ⌫ goes back (or leaves the viewer); ⌥← / ⌥→ and ⌘← / ⌘→ move through history | Done | shortcuts.rs `go.back` (⌫, ⌘←, ⌥←) and `go.forward` (⌘→, ⌥→); in the viewer ⌫ leaves it (app/commands/mod.rs `keyboard`) | local |
| ⌥↑ opens the parent folder | Done | Of the open folder or smart folder | local |
| ⌥⇧N creates a text file from a template; ⌥⇧C sets the folder cover | Done | ⌥⇧N creates a text document; ⌥⇧C sets the folder cover (`edit.folder.cover`) | local |
| ⌘W closes the window | Done | Closes the viewer first, then the window | local |
| ⌘Z undo | Done | app/commands/mod.rs `keyboard` | local |
| ⌘A select all, ⌘C copy | Done | shortcuts.rs:110-112 | local |
| Delete (Windows) / ⌘⌫ trash; ⇧⌘⌫ remove from folder | Done | ⌘⌫ and the forward-delete key trash; ⇧⌘⌫ removes from the folder | local |
| ⌘T add tags, ⇧⌘C / ⇧⌘V copy and paste tags, ⇧D add to last folder, ⌘D duplicate | Done | `organize.tag.*`, `organize.folder.addLast` ⇧D, `edit.duplicate` ⌘D | local |
| ⇧⌘L lock the folder now, ⌥⌘L lock the app | Done | `edit.folder.lock` ⇧⌘L and `app.lock` ⌥⌘L | local |
| Hold Z for the hover preview | Done | ui/grid/mod.rs `hover_preview`, hover.rs | local |
| Global shortcuts: Show Eagle, Bring up Eagle and search, screen captures (⌥⌘E) | Done | `hotkeys.rs` (Carbon `RegisterEventHotKey`): Show falcon, Show falcon and search, the captures with Eagle's ⌥⌘E chord; **off by default** (a deliberate user decision, stays off: Eagle usually owns ⌥⌘E on the user's Mac); a refused chord is shown in orange on its row and re-registered when the switch or chord changes | local |
| Falcon extras: ⇧⌘K collect window, ⌃Tab / ⌃⇧Tab next or previous open library | Done | shortcuts.rs `file.collect.window`, `library.next`, `library.previous` (falcon only) | local |

Viewer and player keys (frames, volume, speed, ` zoom toggle, N comment, M mute, C comment mode) belong to the viewer audit. Falcon has the player table at shortcuts.rs:122-134.

---

### 4. Toolbar

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Show-sidebar button when the sidebar is hidden | Done | ui/toolbar/mod.rs `panels_button` | local |
| Back / Forward buttons | Done | ui/toolbar/mod.rs `toolbar` | local |
| Title for the current view, "Search results", or "N folders selected" | Done | Page, folder or smart folder name stays during a search, then "Search results (N)" when a search or filter narrows the view; clicking the name clears both; path tooltip and ancestor menu | local |
| Right-click the title for an ancestor jump menu | Done | Toolbar title ancestor menu | local |
| Thumbnail size slider with − and + | Done | ui/toolbar/mod.rs `zoom_slider` | local |
| Refresh button in the Random view | Done | Button on the Random page and in folders or smart folders sorted at random, plus R and View ▸ Shuffle Again | local |
| Layout and sort panel button | Done | `app/layout_panel.rs` in Eagle's order (layout, Fit/Fill, sort, Show Name/Info/Extension/Label/Comments/Subfolders, sidebar and inspector, Refresh); no Speed/Quality | local |
| Filter button with an active state | Done | ui/toolbar/mod.rs `toolbar` | local |
| Search field with a scope menu (Tags / URL / Comment / Note) and a search-by-image camera | Partial | Search scope menu on the caret (`search_scope_menu`); the camera is disabled (search by image is online) | local |
| Actions button | Done | Falcon's own implementation of a feature that is a plugin in Eagle (`ai-action`): the lightning button left of the layout button opens the panel under it and is hidden on All Tags, as in Eagle (`ui/toolbar/mod.rs`) | local |
| Plugins button | Missing | Drawn disabled (ui/toolbar/mod.rs `toolbar`) | online/plugin |
| AI Search button | Missing |  | online/plugin |
| Show-inspector button when the inspector is hidden | Done | app/mod.rs `draw` | local |
| "Move duplicates to trash" button on the duplicates page | Done | ui/grid/duplicates.rs `merge_review` (cross-ref) | local |
| Sidebar header "Add" button that opens the New… menu | Done | The + opens the New… menu, next to the Go to Folder button | local |

---

### 5. Sidebar

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Library name and switcher at the top | Done | ui/sidebar/mod.rs `sidebar`, `library_menu`, which adds several open libraries (falcon extra) | local |
| Nav entries: All, Uncategorized, Untagged, Recently Used, Random, All Tags, Trash, each with a count | Done | ui/sidebar/mod.rs `nav_rows`, which adds Duplicates | local |
| Recently Used lists items the user recently opened | Done | Opened items newest first (viewer after a second, external open, Open With, new window, copy), kept per library in app storage as Eagle | local |
| Community entry | Missing |  | online/plugin |
| Show or hide each nav entry (context menu and preferences) | Done | Context menu on All / Untagged / Random and Preferences ▸ Sidebar | local |
| Show or hide the count badges | Done | `Prefs::sidebar_counts` | local |
| Quick Access section | Done | ui/sidebar/mod.rs `sidebar` | local |
| Section headers (Quick Access / Smart Folders / Folders) collapse on click and show a count | Done | Every header folds with a caret and shows its count (`SectionFold`, saved globally as Eagle) | local |
| Smart Folders tree with groups, nesting, icons, colours, counts | Done | ui/organize.rs `smart_section`, `smart_row` | local |
| Folder tree with carets, guide lines, icon and colour tints, counts, lock badge | Done | ui/sidebar/mod.rs `folder_row` | local |
| Double-click a folder to expand or collapse (preference: or rename) | Done | `Prefs::sidebar_double_click`, folders and smart folders | local |
| Select several folders with ⌘ or ⇧ click to see their combined items | Done | `logic::TreeSel` (⌘ toggles, ⇧ ranges among visible rows), `folders_ui`; the grid shows the union under the global sort | local |
| Filter field at the bottom of the sidebar | Done | ui/sidebar/mod.rs `sidebar`. No ⌥⇧F key | local |
| "Go to Folder" button | Done | Sidebar header ⇄ button opening the quick switcher, in Eagle's order (Create Folder, Go to Folder, Hide) | local |
| Resize the sidebar by dragging its edge | Done | 200–480 pt, kept across launches | local |
| Background task status: "Processing" or "Paused" palette analysis with Pause / Resume | Done | Spinner by the library name while the import queue runs, "Processing (N)" / "Paused (N)" tooltip, click pauses and resumes; falcon has no separate palette queue | local |
| Drop zone hint: "Drag and drop folder here to import" | Done | Falcon's own hint text over the grid, the sidebar and folder rows while files are dragged in | local |

---

### 6. Grid layouts and display options

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Four layouts: Waterfall, Justified, Grid, List | Done | logic/layout.rs `place` | local |
| Default layout is Justified | Done | `logic::Layout` default, `logic::layout_for` | local |
| Layout remembered per folder | Done | `logic::layout_key`: per folder or smart folder, else per library (`Saved.layouts`), as Eagle's `eagle.list.layout.<id>` | local |
| Grid layout Fit or Fill option | Done | `Display.fill`, `draw_image_cover`; hover playback still works in Fill | local |
| Thumbnail Speed vs Quality (load originals for large tiles) | Done | Layout panel option, Quality by default as Eagle; large jpg/png/webp/bmp cards show the original up to 1600 px through a bounded `Tier::Card` cache (16 entries) | local |
| Sort-by choices in the layout panel, global and per folder, including Manual, Duration and Random | Done | Global sort with Duration and Random (`OrderBy::ALL`); a folder's own sort adds Manual in the panel (`own_sort_menu`) | local |
| Toggle Show Name under thumbnails | Done | `Display.name` in Waterfall, Justified and Grid (Justified rows get the caption band, `logic/layout.rs`) | local |
| Toggle item info, choosing Dimensions / File Size / Tags / Rating / Date Modified / Date Created | Done | `logic::caption_info` with Eagle's six types and its fallbacks: duration, font weight, bookmark domain, size for documents/3D/design types; in Justified too | local |
| Toggle the file extension in the name | Done | `logic::caption_name`, in Justified too | local |
| Toggle the extension label badge | Done | `Display.ext_badge` | local |
| Toggle the comment-count badge | Done | `Display.comments` on cards and in List rows (26 px gutter) | local |
| Show subfolder contents in a folder | Done | View menu, ⌥⌘7, the layout panel, the strip switch and the folder context menu | local |
| Show sidebar and Show inspector switches in the layout panel | Done | `layout_panel.rs` | local |
| Refresh button in the layout panel | Done | Flushes, re-reads the library from disk, forgets textures and reshuffles | local |
| List layout with configurable columns and click-to-sort headers | Done | `layout_panel::list_header`, `logic::ListColumns`, `header_sort`: Eagle's seven columns, click sorts (writes the folder's sort inside a folder), right-click picks columns | local |
| Subfolder cards above a folder's items, with covers | Done | covers.rs:65-140 | local |
| Collapse the subfolder strip | Done | Click the heading in the card strip and in the List layout's subfolder rows | local |
| Rename a subfolder card by double-clicking its name | Done | Double-click the name on cards and List rows | local |
| Thumbnail background (checkerboard for transparency, etc.) | Done | Card menu Thumbnail Background ▸ None/Black/Gray/White/Checkerboard, item key `background` with Eagle's values (`logic::ThumbBg`, `Action::Background`); drawn in grid, list and viewer | local |
| Hover zoom button and hold-Z preview | Done | ui/grid/mod.rs `zoom_button`, `hover_preview`, hover.rs | local |
| Video, GIF and audio play on hover in the grid | Partial | GIF, animated WebP and AVIF cards play in place while hovered (⌥ keeps them still); video (strip with scrub, bar, time and sound) and audio hover play in Grid, Waterfall, Justified and List through the one grid loop shared by folder, tag, smart-folder and search pages (`hover_media.rs`, 250 ms video delay as Eagle's timer, thumbnail-only trigger, instant stop on leave, scroll or drag). Gap: the sound has never been heard by a person | local |
| Pinned items stay at the top of a folder | Done | `item_ops::pinned_first` in the open folder unless sorted at random; pin badge on cards and list rows, as Eagle | local |
| Manual drag order in a folder | Done | ui/organize.rs `manual_drag` | local |
| Missing-file or long-path hint on broken thumbnails | Done | Warning mark and tooltip on cards and list rows: file not found, path too long (≥ 260 chars, as Eagle) or couldn't be read | local |

---

### 7. Selection

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Click selects one, ⌘-click toggles, ⇧-click selects a range | Done | app/navigation.rs `click` | local |
| Drag a rectangle over empty space to select (rubber band) | Done | `logic::rect_hits`, `marquee_selection`, `edge_scroll` in all four layouts; ⌘ XORs; ⇧ adds (Eagle nearly replaces) | local |
| ⌘A / Esc / click on empty space | Done | app/commands/mod.rs `run_command` and `keyboard`, ui/grid/mod.rs `grid` | local |
| ⇧ + arrows extend the selection | Done | `logic::extend_selection`: ⇧←/→ grow or shrink from the anchor; ⇧↑/↓ only in List layout, as Eagle | local |
| Right-click an unselected item selects it first | Done | ui/grid/mod.rs `grid` | local |
| After moving items out of a folder or to the Trash, the next item is selected | Done | `App::select_after_removal` for trash, restore, delete, Remove from Folder and drops | local |
| Auto-select items after import | Done | `Prefs::auto_select` (on by default): up to 1,000 new items shown in the view | local |
| Selected item is scrolled into view during keyboard moves | Done | app/commands/mod.rs `keyboard`, ui/grid/mod.rs `grid` | local |
| Several items selected: the inspector shows a summary with batch tag, rating and trash | Done | ui/inspector/mod.rs `multi_inspector` | local |

---

### 8. Drag and drop inside the window

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Drag items onto a sidebar folder | Done | `logic::folder_drop` move/add rules with ⌥ and the toast; a locked target asks for its password first; items leave every picked folder, else the open one, as Eagle's `classifySelected` | local |
| Toast after a drop ("N items moved to Folder", with a link to the folder) | Done | `flash_folder` toast with the folder name as a link (`toast.rs` `Toast.link`) | local |
| Drag items onto a Quick Access folder | Done | Same flow as sidebar rows, incl. the password prompt | local |
| Drag items onto a subfolder card | Done | Same `DropOnFolder` path as sidebar rows, with the bulk gate | local |
| Drag items onto the Trash | Done | ui/sidebar/mod.rs `nav_rows` | local |
| Drag folders to reorder or nest, with insertion lines | Done | `ui/organize.rs` `folder_drop`, `organize::folder_drop`: a drag that begins on a picked folder carries every picked one, as Eagle's `draggedFolders`, in one undo step; a drop on a picked folder or inside one is refused. Eagle's drag helper (icon, name, count by the pointer) isn't drawn | local |
| Drag a folder onto the Folders heading to move it to the top level | Done | ui/sidebar/mod.rs `sidebar` | local |
| Drag smart folders to reorder or nest | Done | ui/organize.rs `smart_row` | local |
| Drag Quick Access entries to reorder them | Done | Reorder by dragging; folders and smart folders can be dragged in | local |
| Drag items within a manually sorted folder to reorder them | Done | ui/organize.rs `manual_drag` | local |
| Drag items out to Finder or other apps | Done | ui/grid/drag_out.rs `drag_out` | local |
| Drag files from Finder onto the grid to import into the open folder | Done | app/mod.rs `draw`, app/import.rs `drop_files` | local |
| Drag files from Finder onto a specific sidebar folder | Done | `app/import.rs` `drop_files` hit-tests the drop against folder rows (`drop_pos.rs`); files go into that folder, folders are ignored, as Eagle's `onDropFolder` | local |
| Drag a Finder folder onto the sidebar to import it with its folder tree | Done | `folder_import.rs` plan and `app/import.rs` `import_folder_trees`: a drop on the sidebar's empty space makes a top-level folder tree and opens it; on the grid it imports flat, as Eagle's `onDropContainer` does | local |
| Drop an .eaglepack onto the window | Done | Grid, sidebar folder rows (into that folder) and empty sidebar space (into the open folder), as Eagle's `onDropSidebar` | local |
| Drop an image or URL from a browser | Missing |  | online/plugin |
| Drag tags onto tag groups or the starred list; ⌥ copies instead of moving | Done | `onDropTag`-style moves, ⌥ copies, Starred stars (and moves to the front) as Eagle; a drag carries all picked tags | local |
| Drag ghost showing the item count | Done | ui/grid/drag_out.rs `drag_out` | local |

---

### 9. Window behaviours

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Frameless window with traffic lights; drag the title area; double-click to zoom | Done | main.rs:45-51, ui/widgets.rs `window_drag` | local |
| Remember window size and position | Done | eframe persistence (Cargo.toml `persistence` feature) | local |
| Full screen | Done | ⌃⌘F and Window ▸ Full Screen, plus the green button | local |
| Always on top for the main window | Done | `modes.rs` | local |
| Open items in a separate preview window: own titlebar always-on-top toggle, opacity 10–100%, navigator, zoom presets 5–800%, grayscale, rotate / flip, copy, copy path / link, Reveal in Eagle | Partial | `ui/viewer/preview_window/`: always-on-top, opacity 10–100%, navigator, zoom presets, grayscale (the window's own flag), rotate/flip, Copy, Copy File Path, Copy ▸ Eagle Link, Reveal in Finder, Show in Falcon; every viewer type incl. 3D and web; ⌘O, middle click and the item menu. Partial because the window's rotate, flip, grayscale and sharp zoom are covered by tests but not yet seen on screen (see the Navigator row in the viewers section) | local |
| Middle-click opens an item in a new window (preference) | Done | `Prefs::middle_button` (new window, default, or nothing); never for audio, as Eagle | local |
| Presentation / slideshow mode | Done | `modes.rs`; `ViewportCommand::Fullscreen`, panels and bar hidden | local |
| Closing the window keeps the app running (Dock icon stays); reopen from the Dock | Done | Closing hides falcon (`NSApp hide:`, so the collect and preview windows hide too); the Dock icon or reopening brings it back; ⌘Q quits | local |
| Menu bar status icon: Show Eagle, Capture Area / Window / Full Screen, Quit | Done | `tray.rs` `NSStatusItem`: Show falcon, Capture Area / Window / Full Screen, Quit; General switch; rebuilt on language change | local |
| Lock screen for the whole app, with Touch ID | Done | Password and a Use Touch ID button (`touch_id.rs`) | local |
| Loading screen while a library opens (progress messages) | Done | Every open goes through the queue behind the loading screen: switcher, Open Library, Try Again, Select Another Path, after creating, launch restore; launch arguments, links and opened files wait until it drains | local |
| Resizable inspector | Done | Drag its inner edge; kept across launches | local |
| Several libraries open in one window (falcon only) | Done | app/library.rs `add_workspace`, workspaces.rs (extra) | local |
| Floating collect window (falcon only) | Done | collect.rs (extra) | local |

---

### 10. Themes and appearance

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Themes: Auto, Light, Light Grey, Gray, Dark, Blue, Purple | Done | theme.rs:73-130, prefs.rs:312 | local |
| Auto theme follows the system | Done | theme.rs:365 `follow_system` | local |
| Transparency (vibrancy) effects | Partial | Preferences ▸ General checkbox, persisted as `Prefs::vibrancy`, applied at once (`vibrancy.rs`): NSVisualEffectView behind the `CAMetalLayer`, sidebar and inspector at 75% alpha, everything else opaque; OFF by default, because Eagle's `general.enableVibrancy` only blurs its menus and boxes while its main window stays opaque. Gaps: falcon's effect is on the side panels, not Eagle's menus and boxes, and it has not yet been seen on screen | local |
| UI zoom level | Done | `Prefs::ui_zoom`, applied live | local |
| Interface language | Done | `crate::locale`: Automatic or one of Eagle's eight languages, applied live to every pane, menu (incl. the native menu bar), toast, dialog and viewer; 1,837 keys incl. context keys (`tags::Uncategorized`…); falcon's own translations, machine-written and unreviewed (`locales/REVIEW.md`) | local |
| File-type colours on extension badges | Done | theme.rs:447-468 | local |

---

### 11. Status and notification UI

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Toast messages (rename, remove, rate, copy path, pin and so on) | Done | `toast.rs`: brief, normal, undoable and error kinds with Eagle's durations; errors also go to the log | local |
| UNDO button on toasts | Done | `toast.rs` undoable kind | local |
| Clickable links in toasts (open the folder) | Done | `Toast.link` on the folder add and drop toasts | local |
| Import progress bar with count and time remaining | Done | "Importing 3 of 10 · About 20 s left · name" (hidden while paused); Pause/Resume on the sidebar spinner | local |
| Import errors toast ("N files failed, click for details") opens an error list with retry and reveal | Done | Toast opens the panel: per-file Retry and Reveal (Eagle has only Retry All and click-to-reveal), Retry All, Copy, Clear All; a failed paste can't be retried (temp file gone) | local |
| Dock icon progress bar during long tasks | Done | `dock_progress.rs` from the progress modal and the background import (finished/total, hidden from 97% as Eagle) | local |
| Progress modals for export, eaglepack, thumbnail refresh, empty trash, WebP conversion and similar | Done | Shared modal with Cancel for pack import/export, thumbnail refresh, Empty Trash, Export to Computer and Export in Format (`export::run` takes a cancel flag); WebP conversion has its own confirm and runs per item | local |
| Sound effects (delete item, delete folder, capture) | Done | falcon's own synthesised sounds (`sounds.rs`, no assets) through the viewer's output stream with Eagle's 500 ms debounce. Item and folder deletes play from the action reducer, so every entry point is covered (Trash, Remove from Folder, folders, smart folders, tags, tag-group removal, permanent delete, Empty Trash; Restore is silent). Screenshots follow Eagle's rule in every mode (full screen always, area and window only with their notice, none with the collect window; `screencapture -x` so the system shutter no longer plays on top). Duplicates, a failed import batch and the auto-import notice play under the master switch | local |
| System notifications (import done, capture done) | Done | `notify.rs` posts UserNotifications for import, export and capture done; they need the bundled app (`dev.falcon.app`), which is how falcon is run; clicking one activates falcon | local |
| Warnings: slow disk, NTFS on macOS, library path permissions, localhost / API port | Done | Read-only file, no write permission, slow disk, NTFS, plus a lasting alert when no API port is free or `localhost:<port>` can't be reached | local |
| Tooltips on toolbar buttons | Done | ui/widgets.rs `icon_button` | local |
| Notification Center history panel | Partial | Falcon addition (Eagle 4 has no notification history): Window ▸ Notification Center lists toasts and notices, newest 200 kept in app data; Collect window statuses not included (`notice_history.rs`). Not yet seen on screen | local |

---

### 12. Onboarding and empty states

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Welcome flow: welcome, choose a theme, create or open a library with example names | Done | First-launch welcome (see the Welcome or onboarding row); its browser extension step is online and left out | local |
| No-library screen offers both Create and Open | Done | Create Library and Open Library with a hint (`ui/dialogs/library.rs`), through the sandbox guard | local |
| Tutorial / tips overlay | Partial | Falcon addition (Eagle's tutorial is an online video link): six local tips, shown once after the welcome flow, reopened from Help ▸ Show Tips (`tips.rs`). Not yet seen on screen | local |
| Empty All or folder view: "Drag and Drop" with a hint | Done | Tailored empty states per view | local |
| Empty search: "Nothing Found, maybe another keyword" | Done | Tailored empty state | local |
| Empty Trash text | Done | Tailored empty state | local |
| Uncategorized and Untagged intro and "done" states | Done | Tailored intro and done states | local |
| No tags yet / no uncategorized tags | Done | "No Tags Yet", plus the all-grouped notice in Tag Group mode (`empty_tags_filed`); falcon has no separate Uncategorized tags view | local |
| Duplicates: none found | Done | ui/grid/duplicates.rs `duplicates_page` | local |
| Locked-folder unlock screen | Done | Password field with hint and a Touch ID button whenever the Mac supports it, as Eagle | local |

---

### Missing and Partial local items, ranked by how noticeable they are

1. ~~No application menubar.~~ Fixed, with falcon's own Actions menu (Eagle's is the `ai-action` plugin).
2. ~~Shortcuts missing: T (tags), F (folders), ⌘J Go to Folder, ⇧-arrows, ⌘T and copy/paste tags, ⇧D last folder, ⌘D duplicate, ⇧T always on top, F5, ⌘O new window.~~ Fixed; the Shortcuts row stays Partial only for the 18 Eagle ids with no row, none of them a local macOS feature.
3. ~~Layout, history and rename keys differ from Eagle.~~ Fixed.
4. ~~Dragging items between folders adds them instead of moving them.~~ Fixed, and the drop toast links to the folder.
5. ~~No rubber-band (marquee) selection.~~ Fixed.
6. ~~Item context menu gaps: Rename / batch rename, Copy ▸ (link, folder path, thumbnail, base64, name), Copy / Paste Tags, Duplicate, Remove from this Folder, Open File Location, Pin to Top, New Folder with Selection, Add to Last Used Folder, Share, thumbnail background, custom thumbnail from clipboard, Replace with File, Combine Images.~~ Fixed; Combine Images is a plugin in Eagle 4 and is not built.
7. ~~Grid display options: only Thumbnail Speed / Quality is missing.~~ Fixed.
8. ~~Add to Folders as a searchable picker instead of a flat list (Go to Folder is done).~~ Fixed; the picker is still Partial (see its row).
9. ~~No multi-folder selection in the sidebar.~~ Fixed.
10. ~~Open in New Window (preview window with always-on-top, opacity and navigator), plus main-window Always on Top.~~ Fixed.
11. ~~Presentation / slideshow mode (F5) and grayscale preview.~~ Fixed.
12. ~~Folder context menu gaps: New Folder (sibling), Move Folder dialog, Expand/Collapse submenu, Duplicate, Copy Link, Show Subfolder Content. Also no menu at all on subfolder cards, and no menu for several folders.~~ Fixed.
13. ~~Fixed-width sidebar and inspector; nav entries fixed.~~ Fixed, and every section header folds.
14. ~~List layout: no column header, no column chooser, no sort-by-column.~~ Fixed.
15. ~~Empty states are a single "No items".~~ Fixed, with the welcome flow and the no-library screen.
16. ~~Toast system: no UNDO button, no links, no import error list, no dock progress, no time remaining.~~ Fixed.
17. ~~Drag and drop gaps: files onto a specific sidebar folder, a Finder folder onto the sidebar (keeping its tree), items onto Quick Access or subfolder cards, Quick Access reorder.~~ Fixed.
18. ~~Context menus have no type-to-filter search.~~ Fixed, inside submenus too (see its row for what stays Partial).
19. ~~Trash ▸ Restore All, Recently Used ▸ Clear history, Recently Used by modification time.~~ Fixed.
20. ~~Right-click on the empty grid (layout panel), right-click on the title (ancestor jump), ⌥↑ to the parent folder, R to reshuffle Random and a refresh button.~~ Fixed.
21. Video, GIF and audio hover-play in the grid is built; the rows stay Partial because the sound has never been heard by a person and the video strip is a set of pictures, not the file playing.
22. ~~Window: closing now hides falcon (whole app) and the Dock icon brings it back; Full Screen and the app lock exist.~~ Fixed.
23. Smaller items: vibrancy and the Color Space menu are Partial (not yet seen on screen); the rest is built.

## Viewers and file formats


Scope: every viewer, its controls, where it opens, how each format gets a thumbnail and a preview, and grid hover behaviour. Eagle evidence is from the extracted app (`app/index.html`, `app/app.bundle.js`, the viewer folders, `app/js/*`, `config.js`, `i18n/en.js`). Falcon evidence is from the code only (no docs or ROADMAP trusted).

Paths: `E/` = `eagle-src/app/app/`, `i18n` = `eagle-src/app/i18n/en.js`, `F/` = `crates/falcon-app/src/`, `FL/` = `crates/falcon-library/src/`.

### 0. Where each viewer opens (the key layout finding)

In Eagle, every viewer opens in the same place. Detail mode is a single `content-panel detail-mode` whose left edge is the sidebar width and whose right edge is the inspector width (`E/index.html:1007`), and every viewer type sits inside it as an `ng-switch` branch (`E/index.html:1268-1545`). So images, GIFs, RAW, TIFF, TGA, SVG, video, audio, fonts, text, PDF, 3D models, special formats, web pages and plugin viewers **all open inside the main window with the sidebar and inspector still visible**. A toggle button in the viewer toolbar (`toggle-all-btn`, `appmenu.view>toggleAll`) hides or shows both panels. Separately, "Open in New Window" (`Context.Open.New.Window`, `view.file.opennewwindow` = Cmd+O, middle click by default) opens a standalone preview window (`E/preview-window.html`), and Space opens a lighter "inline" quick preview (`keyspace: preview`, `E/app.bundle.js:33595`).

Falcon now does the same: `fn viewer` draws every viewer in the central panel, replacing the toolbar, filter bar and grid as Eagle's detail mode does (`index.html:554` hides Eagle's toolbar in detail mode).

| Viewer | Eagle placement | Falcon placement | Falcon status |
| --- | --- | --- | --- |
| Image (jpg/png/webp/bmp/avif/heic…) | Inside main window, sidebar and inspector kept (`index.html:1007`, `:1515`) | Central panel between sidebar and inspector (`F/ui/viewer/mod.rs` `fn viewer`, `F/ui/widgets.rs` `viewer_ui`) | Done |
| GIF | Inside main window (`index.html:1306`) | Central panel between sidebar and inspector (`F/ui/viewer/mod.rs` `fn viewer`, `F/ui/widgets.rs` `viewer_ui`) | Done |
| RAW | Inside main window (`index.html:1327`) | Central panel between sidebar and inspector (`F/ui/viewer/mod.rs` `fn viewer`, `F/ui/widgets.rs` `viewer_ui`) | Done |
| TIFF / TGA | Inside main window (`index.html:1398`, `:1419`) | Central panel between sidebar and inspector (`F/ui/viewer/mod.rs` `fn viewer`, `F/ui/widgets.rs` `viewer_ui`) | Done |
| SVG | Inside main window (`index.html:1496`) | Central panel between sidebar and inspector (`F/ui/viewer/mod.rs` `fn viewer`, `F/ui/widgets.rs` `viewer_ui`) | Done |
| Video | Inside main window (`index.html:1343`) | Central panel between sidebar and inspector (`F/ui/viewer/mod.rs` `fn viewer`, `F/ui/widgets.rs` `viewer_ui`) | Done |
| Audio | Inside main window (`index.html:1366`, `:1377`) | Central panel between sidebar and inspector (`F/ui/viewer/mod.rs` `fn viewer`, `F/ui/widgets.rs` `viewer_ui`) | Done |
| Font | Inside main window (`index.html:1388`) | Central panel between sidebar and inspector (`F/ui/viewer/mod.rs` `fn viewer`, `F/ui/widgets.rs` `viewer_ui`) | Done |
| Text (.txt) | Inside main window (`index.html:1393`) | Central panel between sidebar and inspector (`F/ui/viewer/mod.rs` `fn viewer`, `F/ui/widgets.rs` `viewer_ui`), with in-place editing and zoom (`ui/viewer/text.rs`) | Done |
| PDF | Inside main window (`index.html:1301`) | Central area, sidebar and inspector kept (`F/ui/viewer/mod.rs` `fn viewer`, `F/ui/viewer/pdf.rs` `pdf_viewer`) | Done (macOS only) |
| 3D model | Inside main window (`index.html:1443`) | Central panel, interactive for obj/stl/ply/glb/3mf/fbx/dae/3ds and ifc with full geometry through web-ifc, a still only for an ifc file with no drawable geometry | Partial |
| Special formats (PSD, AI, Office…) | Inside main window (`index.html:1448`) | Central panel between sidebar and inspector (`F/ui/viewer/mod.rs` `fn viewer`, `F/ui/widgets.rs` `viewer_ui`), as a QuickLook render | Partial |
| Web page (url/html/mhtml) | Inside main window (`index.html:1491`) | Central panel; live WKWebView for url, html and mhtml on macOS (MHTML unpacked by `falcon_library::mhtml` into a temp folder per view with a file-only content security policy, `web_archive.rs`); the preview window too | Done |
| Toggle sidebar and inspector from inside the viewer | `toggle-all-btn` in the detail toolbar (`index.html:1015`) | Panels button in every viewer bar and the main toolbar (`App::panels_button`, `logic::toggle_all`), ⌥⌘3 and Tab; both-on/both-off like Eagle | Done |
| Separate preview window | `preview-window.html`, `Context.Open.New.Window` | A preview window per item (`ui/viewer/preview_window`) | Done |
| Space quick preview (inline mode) | Inline overlay with prev/next arrows, counter and exit button (`index.html:1242-1254`, `app.bundle.js:33612`) | `Prefs::space` previews the first selected item in the viewer (with counter and arrows), opens Quick Look or pages down | Done |

### 1. Detail viewer shell (shared by all viewer types)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Open an item by double-click or Enter | Done | `F/ui/grid/mod.rs` (double-click), `F/app/commands/mod.rs` `Command::Viewer` (Enter) | local |
| Preference: double-click opens in the default app instead | Done | `Prefs::double_click` (Controls pane) | local |
| Exit with Esc or the back button | Done | `F/app/commands/mod.rs` `Command::Back`, back button `F/ui/viewer/audio.rs` `bar_shell` | local |
| Exit by double-clicking the viewer background | Done | Double-click anywhere in the image viewer closes it, except in crop or comment mode, as Eagle's `dblclickContentPanel`; video, audio, PDF and font don't, as in Eagle | local |
| Position counter "n / total" in the toolbar | Done | `logic::viewer_counter`, drawn after the back button in `media_view::top_bar` | local |
| Previous and next buttons in the toolbar | Done | Bar buttons and arrows share `App::viewer_step`; disabled at the ends | local |
| Previous and next with the arrow keys | Done | `F/app/commands/mod.rs` `viewer_step` | local |
| "Already the first item" and "Already the last item" tips | Done | Status note over the viewer for about 1.2 s (`logic::Edge`) | local |
| Mouse gesture: right-drag left or right moves to the previous or next item | Done | `gesture.rs`: Eagle's swipe rule (over 20 px, within 1 s, no zoom change) in the image viewer, the too-large page and the preview window, with the trail | local |
| Mouse gesture: right-drag up or down zooms | Done | `gesture.rs`: zoom about the press point, Eagle's ×e per 100/150 px, clamped 5–200% | local |
| Right-click in the viewer opens the item context menu | Done | `App::item_menu` reused by the image viewer and the audio/font background; video and PDF have none (Eagle's PDF is an iframe, a click on video toggles playback) | local |
| Middle-click in the viewer opens the plugin panel | Missing | No plugins | online/plugin |
| Pinned plugins and plugin button in the viewer toolbar | Missing | No plugins | online/plugin |
| Open in default app / Open With from the viewer | Done | `F/ui/viewer/open.rs` `viewer_open_button`, used in `F/ui/viewer/audio.rs` `bar_shell` | local |
| "Cannot preview" page with an "Open With Other Application" button | Done | Icon, name, a line of text and an Open With Another App button; the page's menu has Open, Open With and Show in Finder; shown only when neither render nor thumbnail exists | local |
| "Image too large to preview" page (TIFF/HEIC/TGA above the pixel limit) | Done | `logic::too_large_to_preview` (Eagle's 120,000,000 px limit for tif/tiff/heic/heif/hif/tga without a custom thumbnail); `ui/viewer/image.rs` `too_large` with the thumbnail and an Open With button | local |
| Unsupported format suggests a plugin from the plugin center | Missing | No plugins | online/plugin |
| Presentation (slideshow) mode: full screen, panels hidden | Done | F5, View menu, item menu, middle click; Esc leaves and keeps the viewer | local |
| Always on top | Done | ⇧T (`modes.rs`) | local |
| Thumbnail background per item (none, black, gray, white, grid) used in the viewer and grid | Done | `paint_thumb_bg` from the item's `background` key; wins over the transparency-grid preference | local |
| Transparent-area display preference (grid or no grid) | Done | `prefs.transparency_grid`, off by default as Eagle | local |
| Grayscale preview toggle | Done | Image, GIF, video, PDF, font and grid in the main window; ⌥⌘G, View menu and item menu. Each preview window keeps its own flag (see the Navigator row in the viewers section) | local |
| Navigator (minimap) while zoomed | Done | Eagle's 120 px box, 16 px inset, shown by its overflow rule; click centres, drag pans (`logic::navigator_*`); ⌥⌘8 and View ▸ Navigator | local |
| Viewer keys follow the shortcut table (zoom, crop, rotate, flip) | Done | `view.zoom.*`, `edit.image.crop`, `edit.image.rotate` ⇧R, `edit.image.flip` ⇧F (Eagle's ids); the keys act only while a viewer is open | local |

### 2. Image viewer (jpg, png, webp, bmp, avif, heic, jxl, ico and the like)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Fit to window on open ("Smart Zoom") | Done | `logic::smart_fit` | local |
| Preference: open at original size instead | Done | `prefs.open_actual_size` | local |
| Long images (very tall or wide) open fitted to width for scrolling | Done | `logic::smart_fit` with Eagle's thresholds (width ≥ 960 and height > 1.8×, or width ≥ 320 and height > 2.7×); tall images only, as Eagle has no rule for wide strips | local |
| Zoom slider (5–200%) in the toolbar | Done | `ui/viewer/image.rs`: positions 5–200 showing 5–800% (7% per step above 100), snapping as Eagle | local |
| Zoom presets menu (5%–800%, Actual Size, Zoom to Fit) from the % label | Done | `ui/viewer/image.rs` size menu on the % label; right-click toggles actual size and fit | local |
| Zoom in and out buttons and keys | Done | ⌘= and ⌘− act on the viewer while it is open (`logic::view_zoom`); unmodified + and − still work | local |
| Actual size (100%) | Done | "1:1" button, the 1 key and ⌘0 (`view.zoom.actual`) | local |
| Zoom to fit | Done | "Fit" button, the 0 key and ⌘9 (`view.zoom.fit`) | local |
| Fit to window edge toggle (backtick key) | Done | Backtick, saved in `Saved.zoom_edge` | local |
| Pinch zoom around the pointer | Done | `F/ui/viewer/image.rs` `image_canvas`, 5–800% | local |
| Mouse-wheel behaviour preference (scroll, zoom, or previous/next) | Done | `logic::wheel_action`, `prefs.wheel`: scroll (default), zoom or page; no first-use prompt | local |
| Pan by dragging | Done | `F/ui/viewer/image.rs` `image_canvas` | local |
| Remember the last zoom and position per item | Done | `prefs.remember_zoom`, per session as Eagle's `lastItemStates` | local |
| Pixelated or smooth rendering when zoomed in | Done | `logic::pixelated_at`: nearest sampling from 200% as Eagle, viewer and preview window; the texture is remade in the background | local |
| Shows the original at full resolution | Done | `Tier::Original` in `loader.rs`: the original decoded on a worker and cut into ≤ 2048 px tiles within the GPU limit, kept only for the viewed item (`keep_original`); jpg, png, webp, bmp, tiff and RAW previews | local |
| EXIF orientation applied in the viewer | Done | `FL/thumbnail.rs:121-130` | local |
| Rotate | Done | `F/crop.rs:242-250`, Shift+R `F/image_edit.rs:281` | local |
| Rotation mode preference (preview only, or write to file) | Done | `imageRotateMode` preference, default Write; preview mode only changes how the viewer draws | local |
| Horizontal flip (Shift+F), vertical flip | Done | `F/crop.rs:251-258`, Shift-click flips vertically (`F/image_edit.rs:332`) | local |
| Rotate and flip limited to jpg/png/webp/bmp (flip also avif) | Done | `FL/transform.rs:36-53` | local |
| Crop tool (jpg/png/webp) with handles | Done | Crop with handles, ⇧C, W/H fields, Save / Save As / Cancel (`crop.rs`); the edited file replaces the original atomically through `<file>.staged` (Eagle writes in place) and keeps its birth time | local |
| Crop width and height number fields | Done | `crop.rs` bar fields | local |
| Crop "Save As" (keeps the original, adds a new item) | Done | `crop_item_as_new`: same name, tags, folders, URL and rating, dated `modificationTime + 1` (Eagle +0.1); note not carried | local |
| Live crop size readout | Done | `x: y: W: H:` shown while dragging and for a second after a crop key, as Eagle's `showSize` | local |
| Crop keys: arrows move 1 px, ⇧ arrows 10 px, ⌘ arrows resize 1 px, ⌘⇧ arrows 10 px, ⌘S saves | Done | `ui/viewer/crop.rs` `crop_key`, `CropState::take_keys`, read in `app/image_edit.rs` `edit_keys`; W, A, S, D move too, as in Eagle; shrinking stops at Eagle's 24 px | local |
| Comment mode: draw rectangle regions with notes (C key) | Done | `F/comments.rs:201-320`, C key `F/image_edit.rs:277` | local |
| Comment badges, delete X, resize handles, count badge on the toolbar button | Done | `F/comments.rs:213-318` | local |
| Hovering a region shows its note | Done | Tooltip on hover (`F/comments.rs:290`) | local |
| Comment count badge on grid cards | Done | `F/comments.rs:180` (always on; no toggle) | local |
| Default viewer mode preference (preview or comment) | Partial | Falcon addition (Eagle declares `habits.defaultMode` and never reads it): Preferences ▸ Preview ▸ Image; Comment turns comment mode on for the first image of a viewer session. Not yet seen on screen | local |
| Copy, copy path and copy link from the viewer menu | Done | The viewer's right-click menu is the item menu: Copy (files), Copy File Path, Copy ▸ Eagle Link (Eagle's local API link) / Source URL | local |

### 3. GIF and animated images

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| GIF plays in the viewer | Done | `F/anim.rs` streams every frame from disk on a background thread with a bounded frame window (66 MB peak on a 5,000-frame GIF) | local |
| Animated WebP, AVIF and APNG play in the viewer | Partial | WebP and APNG through `anim.rs`; animated AVIF through ImageIO (`anim::Kind::ImageIo`) on macOS only | local |
| Frame-by-frame GIF player toggle | Done | `F/ui/viewer/gif.rs` `gif_toggle_button`; `prefs.gif_player` (off by default, as Eagle's `habits.gifViewer`) | local |
| Play and pause (Space) | Done | `ui/viewer/gif.rs` `gif_keys`, read before the viewer's keys so Space doesn't close it while the player shows | local |
| Previous and next frame ([ and ]) | Done | The rebindable Player keys; ⇧[ / ⇧] step ten frames | local |
| Frame counter (current / total) | Done | Padded counter in the player bar | local |
| Scrub through frames on a progress bar, with a frame thumbnail on hover | Done | `ui/viewer/gif.rs`; scrolling over the bar steps frames | local |
| Playback speed 0.25x–8x | Done | Eagle's eleven steps; ⇧. / ⇧, also change it | local |
| Loop range (A-B) set on the progress bar, and reset | Done | Drag handles at both ends of the bar; Reset in the more menu | local |
| Set current frame as the GIF's thumbnail | Done | `FL/thumbnail.rs` `set_frame_thumbnail` writes `<name>_thumbnail.png` (short side ≤ 480 px) and recomputes the palette, no `customThumbnail` key, as Eagle | local |
| Comments on GIFs | Done | `F/comments.rs:169` allows GIFs | local |

### 4. RAW photos (arw, cr2, cr3, nef, dng, raf, orf, rw2 and others)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| RAW viewer shows the thumbnail, then the embedded full preview | Done | `thumbnail::raw_preview` extracts the largest embedded baseline or progressive JPEG, turned by EXIF orientation; QuickLook fallback on macOS | local |
| EXIF orientation for RAW | Done | TIFF-based files, ORF/RW2, RAF, CR3, and CRW (CIFF), MRW and X3F containers (`exif.rs`); the CRW 90/270 mapping is from format knowledge, not a real file | local |
| Camera metadata (camera, ISO, focal length, shutter, aperture, date) | Done | `F/ui/viewer/audio.rs` `media_props` (when `rawMetas` exists) | local |
| Zoom, comments on RAW | Done | Same image viewer and `F/comments.rs:169` | local |

### 5. Video viewer

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Play native formats (mp4, mov, m4v and others) | Done | AVFoundation player (`F/ui/viewer/video.rs` `open`) | local |
| Fallback player for formats Chromium can't play (mkv, avi, wmv, flv and others) | Partial | `falcon-ffmpeg` player behind `falcon_video::Video` in the viewer and the preview window, over an LGPL decode-only FFmpeg 7.1.5 built by `scripts/build-ffmpeg.sh` into `target/ffmpeg` (CONFIG_GPL 0, no encoders, muxers, network or filters; checked again at run time from each library's licence string) and loaded with `dlopen`, never linked; `scripts/bundle.sh` ships the five dylibs in Contents/Frameworks with the LGPL text in Resources/FFmpeg. Same viewer UI, seek-bar thumbnail, hover strip and hover sound, import thumbnail at Eagle's min(10 s, duration/3) and duration. AV1 decodes through dav1d 1.5.4 (BSD-2, built by the same script with meson/ninja/pkgconf pinned into a venv under target/, shipped as libdav1d.7.dylib). Gaps: pitch moves with speed (atempo lives in avfilter); Eagle's 1.5 s no-frame switch is not copied; without a built FFmpeg the can't-play page says the libraries weren't found | local |
| "Can't play this file" page with an Open With button | Done | `ui/viewer/video.rs` `cannot_play`, `cannot_play_line(ext, ffmpeg)`: thumbnail, name, a line saying neither macOS nor FFmpeg could play the file (or that the FFmpeg libraries weren't found), Open With | local |
| Autoplay on open | Done | `F/ui/viewer/video.rs`; switch `Prefs::video_autoplay` | local |
| Play/pause button, click picture to toggle, Space | Done | `F/ui/viewer/video.rs` `video_view`, `video_keys`, `F/shortcuts.rs:122` | local |
| Seek bar with drag scrubbing | Done | `F/ui/viewer/video.rs` `video_view` | local |
| Hover thumbnail on the seek bar (preview frame under the pointer) | Done | `SeekPreview` fed by `falcon_video::Grabber` on a worker thread; the grabber falls back to FFmpeg for formats macOS can't read | local |
| Skip back and forward buttons (2%, capped at 10 s / 20 s), hold to repeat | Done | `Repeat`: acts on press, repeats every 100 ms while held; 2% capped at 10 s back / 20 s forward | local |
| Frame step back and forward, 1 and 10 frames | Done | Buttons and [ ] Shift+[ Shift+] (`F/ui/viewer/video.rs` `video_view`, `F/shortcuts.rs:123-126`) | local |
| Playback speed menu 0.25x–8x and Shift+. / Shift+, | Done | `F/ui/viewer/video.rs` `video_view`, `F/shortcuts.rs:131-132` | local |
| Loop toggle, remembered | Done | `F/ui/viewer/video.rs` `video_view` | local |
| Always loop clips shorter than 30 s | Done | `F/ui/viewer/video.rs`; switch `Prefs::video_loop_short` | local |
| Volume slider, mute, volume keys (Cmd+Up/Down), remembered | Done | `F/ui/viewer/video.rs` `video_view`, `F/shortcuts.rs:127-128` | local |
| Volume / seek tip overlay while adjusting | Done | `volume_tip`, top right for 1 s; Eagle has no seek tip either | local |
| Wheel over the picture seeks; Shift/Alt+wheel seeks horizontally | Done | `F/ui/viewer/video.rs`; `Prefs::video_scroll` can make the wheel change the volume instead | local |
| Preference: vertical wheel controls volume instead | Done | `VideoScroll::Volume` preference, applied in `ui/viewer/video.rs` | local |
| Remember the last playback position | Done | `Prefs::video_resume`; positions saved per item id in app state (`Positions`, latest 500), across launches | local |
| Stretch video to fill the window | Done | `Prefs::video_fill` | local |
| Full-screen button | Done | Enters presentation mode, as Eagle's does | local |
| Flip and rotate the video (viewer toolbar) | Done | `rotate_step`, `corner_uvs`; view only, also ⇧R / ⇧F (⇧-click rotates clockwise) | local |
| Timed notes: add a note at the current time | Done | `F/ui/viewer/video.rs` `timed_comment_editor`, N key `F/shortcuts.rs:134` | local |
| Timed notes shown as markers on the seek bar; click to jump, edit, delete | Done | `F/ui/viewer/video.rs` `video_view`, `marker_at` | local |
| Set current frame as the video thumbnail | Done | `Library::set_video_thumbnail`: `thumbnailAt` kept in metadata, 480 px JPEG under `<name>_thumbnail.png`, palette recomputed, as Eagle | local |
| Copy current frame to the clipboard | Done | `frame_tool`, ⇧C (full decoded frame; Eagle copies at the recorded size) | local |
| Save current frame as an image | Done | ⇧S imports a `<name> - <t>` JPEG as a new item with the video's URL and folders, as Eagle | local |
| Load subtitles (.srt, .vtt) | Done | `F/subtitles.rs` parser (no crate); sidecar `<name>.vtt/.srt` in the `.info` folder, Load Subtitles… copies one in, as Eagle | local |
| Open in default app from the video toolbar | Done | `F/ui/viewer/open.rs` `viewer_open_button` | local |

### 6. Audio viewer

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Waveform display | Done | `waveform::peaks` on `falcon_library::audio`, the same decoder as playback, for Eagle's six formats only (`ui/viewer/audio.rs`) | local |
| Click or drag the waveform to seek | Done | `F/ui/viewer/audio.rs` `audio_view` | local |
| Autoplay on open | Done | `F/ui/viewer/audio.rs` `audio_view` | local |
| Play/pause button and Space | Done | `F/ui/viewer/audio.rs` `audio_view`, `F/ui/viewer/video.rs` `video_keys` | local |
| Time readout | Done | `F/ui/viewer/audio.rs` `audio_view` | local |
| Loop toggle | Done | `F/ui/viewer/audio.rs` `audio_view` | local |
| Volume slider | Done | `F/ui/viewer/audio.rs` `audio_view` | local |
| Volume keys (Cmd+Up/Down) | Done | `ui/viewer/video.rs` `audio_action`: the Player key table drives the audio player | local |
| Skip back and forward keys (Cmd+Left/Right) | Done | `audio_action` | local |
| Playback speed (menu and Shift+. / Shift+,) | Done | `Audio::set_speed`, `MENU_RATES` 0.5–2× in the menu, 0.25–8× by key; pitch follows speed | local |
| m4a playback | Done | `falcon_library::audio` (symphonia isomp4): AAC-LC, ALAC, FLAC and Opus in m4a with the edit list and iTunSMPB priming trimmed, through `audio.rs` `Stream` | local |
| BPM and duration in the inspector | Done | `F/ui/viewer/audio.rs` `media_props` | local |

### 7. Font viewer

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Specimen drawn in the font itself | Done | `F/ui/viewer/font.rs` `font_view` | local |
| Editable sample text | Done | `F/ui/viewer/font.rs` `font_view` | local |
| Size slider with zoom-in and zoom-out buttons | Done | `F/ui/viewer/font.rs`: 50–1000% slider with − and +, ⌘=/⌘−/⌘0/⌘9, modifier-wheel and pinch | local |
| Tabs: Article, Waterfall, Alphabet, Other (info) | Done | `ui/viewer/font.rs` tabs Article / Waterfall / Alphabet / Info, remembered in app state | local |
| Colour themes for the specimen (auto, light, dark, purple, yellow) | Done | auto, light, dark, purple, yellow | local |
| Font info panel (PostScript, full name, family, version, designer, maker, description, copyright, trademark, glyph count) | Done | `fontmeta::FontInfo`, `info_rows`; also for fonts egui can't parse, around a QuickLook specimen | local |
| Rename the file from the PostScript, full or family name | Done | `Op::Rename` from the info panel or header; also for fonts egui can't parse | local |
| Activate or deactivate (install) the font | Partial | `font_install.rs` into `~/Library/Fonts/Falcon/` (Eagle's folder left alone), with a confirmation Eagle doesn't show; also for fonts egui can't parse | local |
| Change the sample language | Done | Language menu writing `fontMetas.preferLng` (`Library::set_font_language`), redrawing the specimen thumbnail | local |
| Number keys rate the font from inside the viewer | Done | `digits_rate` in `keyboard`: font, video, audio and text viewers rate; the image viewer keeps 0/1 for zoom | local |
| Fonts egui can't read (some woff2, CFF) | Done | Font-viewer header (rename, Activate, PostScript name), a Specimen tab (QuickLook render, else the thumbnail) and an Info tab | local |

### 8. Text (.txt) editor

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| View text in the app | Done | `ui/viewer/text.rs` `text_viewer` in the central viewer panel, `textdoc.rs` rules; non-UTF-8 or files over 2 MB open read-only | local |
| Edit the text in place, auto-saved | Done | `Library::save_text` (`crates/falcon-library/src/text.rs`) writes atomically 500 ms after typing stops, on blur, ⌘S and leaving; refreshes `size`, `mtime`, `lastModified`; empty text is never saved, as Eagle | local |
| Edit the title (rename) at the top of the page | Partial | `Action::RenameItem` on blur/Enter; Eagle 4.0.0 ships this commented out, so it is a documented difference | local |
| Zoom the text size (wheel and buttons) | Done | Eagle's five sizes 14–42 pt; ⌘=/⌘−, ⌘0/⌘9, ⌘-wheel/pinch and bar buttons; size kept in `Saved.text_zoom` | local |
| Reloads when the file changes outside | Done | One `stat` a second, reload on an mtime change unless typing is pending; F5 reloads | local |
| Links in the text open in the browser | Done | http(s)/www. links underlined, ⌘-click opens through `open_with::open_url`; Eagle's handler never fires | local |
| Number keys rate the item from inside the editor | Done | 0–5 when neither the text nor the title has focus | local |

### 9. PDF reader (pdf.js)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Opens inside the main window with sidebar and inspector visible | Done | `F/ui/viewer/mod.rs` `fn viewer`, `F/ui/viewer/pdf.rs` `pdf_viewer` | local |
| Works on Windows and Linux too | Missing | Out of scope: falcon targets macOS (PDFKit, AVFoundation, QuickLook, ImageIO) | local |
| Continuous vertical page scrolling | Done | `F/ui/viewer/pdf.rs` `pages` | local |
| Previous and next page buttons, n/j/p/k keys | Done | `F/ui/viewer/pdf.rs` `toolbar`, `key_action` | local |
| Page number field and page count | Done | `F/ui/viewer/pdf.rs` `toolbar` | local |
| First and last page (Home, End) | Done | `F/ui/viewer/pdf.rs` `key_action` | local |
| Zoom in and out buttons and Cmd+= / Cmd+- / Cmd+0 | Done | `F/ui/viewer/pdf.rs` `toolbar`, `key_action` | local |
| Zoom menu: Automatic, Actual Size, Page Fit, Page Width, 50%–400% | Done | `F/ui/viewer/pdf.rs` `toolbar` | local |
| Pinch or Cmd+wheel zoom around the pointer | Done | `F/ui/viewer/pdf.rs` `pages` | local |
| Sidebar with page thumbnails | Done | `F/ui/viewer/pdf.rs` `side_panel` | local |
| Sidebar outline (bookmarks) | Done | `F/ui/viewer/pdf.rs` `side_panel` | local |
| Highlight the outline entry for the current page | Done | `ui/viewer/pdf.rs` outline follows the page and scrolls to it | local |
| Sidebar attachments list | Done | `falcon_pdf::Document::attachments` through CGPDF (EmbeddedFiles tree and file annotations); click saves the file | local |
| Sidebar layers (optional content) | Partial | A checkbox per layer redraws pages and thumbnails, Reset Layers returns to the default and each document starts at its own default (`Document::set_layers`, an in-memory incremental update PDFKit draws; pixel tests on generated two-layer PDFs: table, stream and encrypted files, and through the reader's worker). With an `/Order` the sidebar shows pdf.js's tree (`falcon_pdf::layer_tree`: labelled groups with no checkbox, PDFDocEncoding and UTF-16 labels, arrays after a group nested under it as the PDF spec says (pdf.js drops an unlabelled array and lists its groups under Additional layers), unknown references skipped, depth capped at pdf.js's 10, unlisted groups last under "Additional layers", no cascade from a parent), flat otherwise; tests on generated PDFs cover table, stream and encrypted files, cycles and missing Order. Printing hands `falcon_pdf::print` the chosen layers and prints the same in-memory update (`open_for_print`; pixel-tested without a print panel; the file on disk is untouched). Gap: the sidebar tree and the print panel haven't been seen on screen | local |
| Find bar with previous/next, match count | Done | `F/ui/viewer/pdf.rs` `find_bar` | local |
| Find option: match case | Done | `F/ui/viewer/pdf.rs` `find_bar` | local |
| Find option: highlight all | Done | All hits always drawn (`F/ui/viewer/pdf.rs` `pages`); no toggle | local |
| Find options: whole words, match diacritics | Done | `falcon_pdf::is_whole_word`, find options menu | local |
| Select text and copy it | Done | `F/ui/viewer/pdf.rs` `pages`, copy in `pdf_keys` | local |
| Hand tool (drag to pan) | Done | Tools menu; keys s / h as pdf.js | local |
| Rotate pages clockwise and counter-clockwise | Done | r / ⇧R, view only; find hits and selections follow | local |
| Scroll modes: page, vertical, horizontal, wrapped | Done | Tools menu; page mode turns with n/p or the wheel at the page end; remembered per document | local |
| Spread modes: none, odd, even (two-page) | Done | Tools menu; Page Fit and Page Width fit the spread | local |
| Presentation mode (full screen) | Done | Tools ▸ Presentation Mode uses the app's presentation mode | local |
| Print | Done | ⌘P / Tools ▸ Print… through `falcon_pdf::print` (`NSPrintOperation`, main thread, re-opened with the document's password) | local |
| Document properties dialog | Done | Tools ▸ Document Properties: file, size, metadata, dates, version, pages, page size with paper names, fast web view, password | local |
| Password-protected PDFs | Done | `Document::open_with_password` (`unlockWithPassword`); the worker asks the UI and retries | local |
| Clickable internal and external links | Done | `Document::links` from PDFKit annotations: GoTo and URL actions; only http, https and mailto open | local |
| Remember page and zoom per document | Done | `F/ui/viewer/pdf.rs` `mark`, `set_marks` | local |
| ←/→, Space and Esc still move between items or close | Done | `F/ui/viewer/pdf.rs` `pdf_keys` | local |
| Download, Open File, draw and free-text editing | n/a | Hidden in Eagle too | local |

### 10. 3D model viewer (Online 3D Viewer: fbx, obj, 3ds, 3mf, dae, ifc, ply, stl, glb)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Interactive 3D view (orbit, pan, zoom) | Partial | `falcon_library::model3d` readers obj+mtl, stl, ply, glb, 3mf, fbx (binary and ASCII via ufbx, world transforms, material colours, unit and axis conversion, bind pose), dae (COLLADA 1.4.1/1.5 via roxmltree: scene transforms and instances, controllers in bind pose, up axis and unit to Y-up metres), 3ds (material groups, every colour form, smoothing-group normals; keyframer skipped) and ifc (web-ifc 0.0.78, MPL-2.0, built from fetched source by `crates/falcon-ifc` and run in a separate worker process with stack, time and memory limits, so a bad file cannot take falcon down; 117 buildingSMART and web-ifc samples read with no fallback, a superset of the products Eagle's own engine draws; falcon's tessellated reader remains the fallback when the worker is absent or fails); orbit, pan, zoom, wireframe, grid and axes in `ui/viewer/model.rs`. Textures: glb (base colour, KHR_texture_transform, MASK cutoff), obj (`map_Kd`), fbx (embedded or sidecar), dae (`library_images` 1.4.1/1.5, embedded hex) and 3ds (0xA200 with tiling, scale and offset) draw their diffuse texture in the viewer (textured pipeline, mipmaps, anisotropic filtering, the file's wrap modes), in snapshots and in import thumbnails, within 4096 px per texture and 64 MB per model; a missing texture falls back to the material colour. Normal, bump and emissive maps and alpha blending follow what Eagle's viewer draws per format (glb normal, emissive, BLEND and MASK; obj `map_bump`, `d`, `Tr`; fbx normal, bump, opacity; dae `bump` as a normal map, `transparent` and `transparency`; 3ds and ifc transparency) in the viewer, snapshots and thumbnails, within the same 64 MB, with tests on a real GPU. Gaps: those maps and blending have not yet been compared by eye with Eagle, and Eagle always repeats textures where falcon follows the file's wrap mode; IFC products built only from `IfcSectionedSpine`, `IfcBoxedHalfSpace`, `IfcRevolvedAreaSolidTapered`, `IfcMirroredProfileDef` or toroidal/spherical/conical advanced-brep faces are skipped with the type named, as in web-ifc itself | local |
| Fit model to window | Done | Fit button (⌘0), `viewer3d::fit` | local |
| Up-axis (Y/Z), flip up vector, fixed or free orbit | Done | Settings menu: Y/Z up, Flip, Fixed Up Vector / Free Orbit | local |
| Perspective or orthographic camera | Done | Settings menu: Perspective / Orthographic | local |
| Measure tool | Done | Distance, angle and distance between parallel faces (`ui/viewer/model.rs`) | local |
| Model tree navigator (files, meshes, materials) and details sidebar | Done | Files, meshes (hide, isolate, fit one), materials, plus the details panel | local |
| Settings: background colour, edge display, dark theme | Done | Background colour, edges with colour and angle, default colour, Auto/Light/Dark theme, Reset | local |
| Export model and create snapshot | Done | Viewer menu exports GLB, glTF, OFF, OBJ+MTL, PLY and STL (`export::Format::ALL`, round-trip tests incl. OFF); the snapshot draws edges, grid and axes; every format is in Online 3D Viewer's export list (its Dotbim .bim is not built either). 3DM is not built: proprietary Rhino format | local |
| Esc/←/→ still exit or move between items | Done | The shared viewer shell handles Esc, ← and → | local |

### 11. Special formats viewed as images (PSD, PSB, AI, EPS, XD, Sketch, Figma, Affinity, C4D, Blender, SketchUp, DWG, CDR, Clip, InDesign, Office, iWork, XMind, MindNode, OmniGraffle, HDR, EXR, ICO, ICNS, DDS)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Shown as a zoomable image of the thumbnail | Done | Image viewer falls back to the preview tier (`F/ui/viewer/image.rs` `image_canvas`) | local |
| Higher-resolution render made on demand for PSD, PSB, AI, PPT, PPTX, POTX | Partial | PSD, PSDT and PSB: the merged image is decoded by falcon's own reader of Adobe's spec (`falcon_library::psd`: raw and RLE, 8/16/32 bit, bitmap/gray/indexed/RGB/CMYK/duotone/Lab, ICC colour through moxcms, alpha only when the layer section flags it) into the tiled original path up to 16384² and the on-screen rows beyond, with no 8192 px cap; QuickLook stays the fallback when the file has no composite, with the reason in the inspector. AI and AIT saved PDF-compatible (by their `%PDF` header): PDFKit draws the part on screen at the size shown after the view rests 0.4 s, with no cap and one patch held, lined up with QuickLook's picture (crop box, rotation, white). PostScript-only AI, PPT, PPTX and POTX: re-rendered through QuickLook at the shown size after the same rest, visible part only. The separate preview window now asks for the same renders as the main viewer (`ask_sharp` in `ui/viewer/image.rs`, on the window's own loader), not while the window's view is turned and not for animations; tests cover the jobs and region asked, but this has not yet been seen on screen. Gaps: PPT, PPTX and POTX and PostScript-only `.ai` files keep the QuickLook 8192² cap (QuickLook renders whole pages only). Checked for PowerPoint files on macOS 27: `qlmanage -p` gets `public.html` from `Office.qlgenerator`, not PDF, and `QLThumbnailGenerator` returns bitmaps, so PDFKit has nothing to draw by region; a web view and print route isn't a modest change | local |
| Comments on special formats | Done | `F/comments.rs:169` | local |
| Custom thumbnail shown in the viewer for formats with no preview | Done | The custom thumbnail is the viewer's picture for formats with no picture of their own | local |
| HDR/EXR rendered with tone mapping and exposure | Done | `falcon_library::hdr`: linear decode, Reinhard tone mapping as Eagle (fixed exposure 1.0 there); falcon adds an exposure slider | local |

### 12. SVG, TIFF, TGA, HEIC

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| SVG drawn as vectors, sharp at any zoom | Done | `thumbnail::render_svg_region` re-renders the visible part at screen resolution 0.15 s after the view holds still (`logic::svg_region`) | local |
| TIFF viewer (multi-page aware, worker decode) | Done | `tiff_pages.rs`: pages counted on a worker, `‹ 2 / 5 ›` in the bar, PageUp/PageDown; Eagle's own TIFF viewer decodes only the first page | local |
| TGA viewer | Done | Full resolution and tiles through the `image` TGA decoder; the too-large rule kept | local |
| HEIC/HEIF viewer | Partial | Full resolution, orientation and tiles through ImageIO (`crates/falcon-library/src/imageio.rs`) on macOS; QuickLook still elsewhere | local |

### 13. Web pages (url, html, mhtml)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Live web page inside the viewer | Done | `webview.rs` / `ui/viewer/web.rs`: a WKWebView over the central panel on macOS (http/https only); still mode elsewhere | online/plugin |
| Back, forward, reload, copy URL, open in browser | Done | The web viewer bar on macOS (title, URL, back, forward, reload/stop, Copy URL, Open in Browser) | online/plugin |
| Local .html/.mhtml rendered | Partial | HTML loads live in the WKWebView; MHTML is a QuickLook still with Open in Default App (WebKit on macOS can't read it) | local |

### 14. Plugin viewers

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Format plugins provide thumbnails and viewers for extra formats | Missing | No plugin system | online/plugin |

### 15. Separate preview window ("Open in New Window")

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Open one or more items in their own window (Cmd+O, middle click) | Done | ⌘O and the middle-click preference open the acted-on items in a window that steps with ←/→ (`open_preview_items`), as Eagle's | local |
| Same viewers in the window (image, GIF, RAW, video, font, text, PDF, 3D, web, plugin) | Done | Image, GIF, video, audio, text, PDF, fonts, 3D (shared wgpu callback) and web (WKWebView on the preview's NSWindow) in the preview window; plugins not applicable | local |
| Window opacity 10%–100% | Done | NSWindow `alphaValue` from the window menu | local |
| Always on top toggle in the window | Done | Preview window menu | local |
| Navigator, grayscale, rotate, flip in the window menu | Partial | Preview window menu (`ui/viewer/preview_window/mod.rs`). In write mode (`RotateMode::Write`, the default, Eagle's `imageRotateMode`) rotate and flip rewrite the file through the main viewer's `Op::Rotate` and `Op::Flip` for formats `transform::can_rotate` and `can_flip` accept; in preview mode, or for a format falcon can't write, only the window's view turns. A video's picture turns and mirrors in the view only, never the file. The keys `edit.image.rotate`, `edit.image.flip` and `view.grayscale` are bound in the window. Audio never opens in a preview window, in Eagle or falcon. Grayscale is the window's own flag (`PreviewWin::gray`), off at start and kept while stepping items, on a loader of the window's own (`PreviewWin::loader`), as the main window's `modes.grayscale` stays its own and grays only its grid and viewer, as in Eagle. Tests cover the write-or-view rule, a turned view asking for no sharp render, grayscale surviving a step and the loader following forgets, but none of this has been seen on screen. Gaps: View ▸ Grayscale in the app menu bar always acts on the main window (only the key and the window's right-click menu act on a preview window); the web and 3D panes are not grayed in either window, where Eagle's preview CSS grays iframes | local |
| Copy, copy path, copy link, reveal in Finder, open in default app | Done | Copy, Copy File Path, Copy ▸ Eagle Link, Reveal in Finder, Open in Default App | local |
| Copy or save the current video frame | Partial | Preview window video player: Copy Frame as Eagle; Save Frame… writes a PNG through a file dialog, where Eagle's `videoScreenShot` (`E/js/preview-window.js`) adds a JPEG item to the library | local |
| Reveal in Eagle (select the item in the main window) | Done | Show in Falcon | local |

### 16. Grid hover behaviours

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Hover zoom button on cards, enlarged preview after 200 ms | Done | `F/hover.rs:10`, `F/ui/grid/mod.rs` `hover_preview` | local |
| Hold Z over a card for an instant preview | Done | `F/ui/grid/mod.rs` `hover_preview`, `F/hover.rs:397` | local |
| Preview loads the original for raster formats | Done | `F/hover.rs:26`, `F/ui/grid/mod.rs` `hover_preview` | local |
| Animated GIF plays inside the hover preview | Done | `ui/viewer/gif.rs` `hover_anim`, `anim.rs` `Cards::popup` | local |
| GIF and animated images play when hovered in the grid | Done | GIF, animated WebP and (macOS) animated AVIF (`card_anim`, `Cards::play`) | local |
| Preference: always autoplay GIF/WebP/AVIF in the grid | Done | `prefs.always_play_animated` (off by default, as Eagle); AVIF on macOS | local |
| Video hover preview: autoplays, moving the pointer scrubs, progress bar and time | Partial | Picture strip stepped in real time after 250 ms, thumbnail-only trigger, pointer scrub with 200 ms resume, bar and 600 ms time label, in all four layouts (List shows the bar only on small thumbnails); remembered mute; sound through `falcon_library::audio` (symphonia, then the FFmpeg backend) following the scrub with the drift/reseek rule, one sound at a time; the grabber falls back to FFmpeg when AVFoundation can't decode (AV1 MP4). Gaps: not heard by a person; Matroska/WebM/AVI/WMV/FLV/TS and 3GP-AMR sound needs the FFmpeg libraries; the strip is a set of pictures, not the file playing | local |
| Mute toggle on the video hover preview, remembered | Done | `Display::video_muted`, muted by default as Eagle; sound through the shared hover audio player when unmuted | local |
| Audio hover preview: plays, click to seek, progress | Done | Plays after 200 ms, click to seek, progress; stops when the window hides or is minimised; every audio format falcon decodes, with the decoded length when the record has none | local |
| YouTube, Vimeo and Bilibili hover previews for bookmarked links | Missing | None | online/plugin |
| Text files show their text on the card | Done | Eagle's `text` key written at import in Eagle's key order (first 32 Ki UTF-16 units), updated on save, drawn as name plus up to 50 lines; `.txt` only, as Eagle | local |

### 17. File formats

Eagle columns come from `config.js` (`SUPPORT_FORMATS_DARWIN`), `E/js/thumbs/thumb.js:94-309` and the viewer switch in `E/index.html`. Falcon columns match `FORMATS.md`, which is the maintained list; this table is kept in sync with it as rows land.

| Format | Eagle viewer | Falcon thumbnail | Falcon viewer |
| --- | --- | --- | --- |
| jpg, jpeg, jfif, jpe, insp | Image viewer, full res | Decoded | Image viewer, full resolution, tiled for large images |
| png | Image viewer | Decoded | Image viewer (APNG animates) |
| webp | Image viewer, animates | Decoded (first frame) | Image viewer, animates |
| gif | GIF viewer with frame player | Decoded (first frame) | Animated, all frames, frame player |
| bmp | Image viewer | Decoded | Image viewer |
| svg | Vector image viewer | Rendered (resvg) | Vector viewer, redrawn sharp at every zoom |
| avif | Image viewer, animates | QL | Image viewer, full res, animates (macOS ImageIO) |
| jxl | Image viewer | QL (macOS 14+) | Image viewer, full res (ImageIO, macOS 14+) |
| heic, heif, hif | Image viewer | QL | Image viewer, full res (ImageIO, macOS) |
| tif, tiff | TIFF viewer | QL | Image viewer, full resolution, multi-page paging |
| tga | TGA viewer | QL | Image viewer, full resolution |
| ico | Image of thumbnail | QL | QuickLook render shown in the viewer |
| icns | Image of thumbnail | QL | QuickLook render shown in the viewer |
| dds | Image of thumbnail | QL | QuickLook render shown in the viewer |
| hdr | Image of thumbnail | QL | Tone-mapped viewer with an exposure slider (`falcon_library::hdr`) |
| exr | Image of thumbnail | QL | Tone-mapped viewer with an exposure slider (`falcon_library::hdr`) |
| RAW: 3fr arw cr2 cr3 crw dng erf mrw nef nrw orf pef raf raw rw2 sr2 srw x3f | RAW viewer (embedded full preview) | Embedded camera preview at import (crw, mrw, orf, raf, raw, srw, x3f included), else QL | Embedded full-size preview, correctly oriented. Gap: mrw and raw files with no embedded JPEG get no thumbnail (needs a RAW decoder) |
| psd, psdt, psb | Native high-res viewer | QL (size from the file header) | Merged image decoded by falcon at full resolution with ICC colour, tiled; QuickLook when the file has no composite |
| ai, ait | Native high-res viewer | PDFKit, QL | PDF-compatible files: PDFKit draws the part on screen at the size shown, at any zoom. PostScript-only files: QuickLook render, redrawn at each zoom level up to 8192 px Falls back to page 1 through PDFKit, then the PostScript preview, when QuickLook has none. |
| eps | Image of thumbnail | QL | QuickLook render (macOS 14+ has no EPS renderer) Falls back to the DOS header TIFF, `%%BeginPreview` or `%AI7_Thumbnail`; an EPS with no preview gets no thumbnail (no PostScript interpreter). |
| xd | Image of thumbnail | QL if available | QuickLook render where the app's generator is installed |
| sketch | Image of thumbnail | In-file preview (QL first) | QuickLook render where the app's generator is installed |
| fig | Image of thumbnail | QL if available | QuickLook render where the app's generator is installed |
| af, afdesign, afphoto, afpub | Image of thumbnail | QL if available | QuickLook render where the app's generator is installed |
| c4d, blend, skp, dwg, cdr, clip, prd, skt | Image of thumbnail | QL if available | QuickLook render where the app's generator is installed blend, skp and dwg read their own preview first (not zstd blend or WMF dwg). |
| pxd, graffle, mindnode | Image of thumbnail | QL if available | QuickLook render where the app's generator is installed |
| xmind | Image of thumbnail | In-file preview (QL first) | QuickLook render where the app's generator is installed |
| eddx, emmx | Image of thumbnail | QL if available | QuickLook render where the app's generator is installed |
| indd, indt, idml | Image of thumbnail | QL if available | QuickLook render where the app's generator is installed Read the XMP thumbnail first. |
| doc, docx | Image of thumbnail | QL | QuickLook render |
| xls, xlsx | Image of thumbnail | QL | QuickLook render |
| ppt, pptx, potx | Native high-res viewer | QL | QuickLook render, redrawn at each zoom level up to 8192 px; no PDF route past that (see the higher-resolution row) |
| key, numbers, pages | Image of thumbnail | QL, else the preview picture inside the package | QuickLook render. Gap: package-style key files with no preview picture get no thumbnail |
| pdf | pdf.js reader in main window | QL | PDFKit reader in main window (macOS) |
| txt | Editable text editor | None | Editable text with autosave (`textdoc.rs`) |
| ttf, otf, ttc, woff, woff2 | Font viewer | Drawn specimen 600×600 | Font viewer (falls back if egui can't read the font) |
| mp3, wav, ogg, flac, aac, m4a | Audio player with waveform | Drawn waveform 280×140 | Audio player with waveform; all decode through `falcon_library::audio` (ADTS with MPEG-2 headers and Opus in Ogg included); other sound files are plain items, as in Eagle |
| mp4, mov, m4v, 3gp, ts, mts, m2ts | HTML5 video player, mpv fallback | QL | AVFoundation player; FFmpeg player for what it refuses (ts, mts, m2ts) or can't decode (AV1 through dav1d) |
| webm, mkv, avi, wmv, mpg, flv, f4v | HTML5 player or mpv fallback | QL if available | AVFoundation if it can, else the FFmpeg player when installed (AV1 included, through dav1d); FFmpeg thumbnail and duration at import |
| 360, afx, eva, vap | Video player | QL if available | AVFoundation, else FFmpeg if it reads the container |
| glb, obj, fbx, 3ds, 3mf, dae, ifc, ply, stl | Interactive 3D viewer | QL if available (obj/stl/ply/dae usually; fbx/ifc/glb rarely) | Interactive viewer with textures (glb, obj, fbx, dae, 3ds), normal, bump and emissive maps and transparency where Eagle's viewer draws them, and drawn, textured thumbnail for glb, obj, fbx, 3mf, ply, stl, dae, 3ds and ifc (full geometry through web-ifc in a worker process); QL still only for an ifc file with no drawable geometry; `.gltf` is a plain item with QuickLook, as in Eagle |
| url | Live web view | QL (generic) | Live WKWebView (macOS); `.url` addresses read on import; `.webloc`, `.htm` and `.mht` are plain items, as in Eagle |
| html | Live web view | QL | Live page in the viewer (WKWebView) |
| mhtml | Live web view | QL | Live page in the viewer and preview window (unpacked by falcon's own MIME reader, no network); 16:10 thumbnail from the page at import |

### 18. Ranked list of local Missing and Partial items

Ranked by how often a user would notice them.

1. ~~**Viewers cover the whole window (Missing).** In Eagle, every viewer opens between the sidebar and inspector (`index.html:1007`). Falcon does this only for PDFs. This affects image, GIF, video, audio, font, text, 3D and special-format viewing.~~ Fixed.
2. ~~**No toggle for sidebar and inspector inside the viewer (Missing)** (`toggle-all-btn`).~~ Fixed.
3. ~~**Image resolution capped at 3072 px (Partial).** Zooming into large photos and SVGs is soft (`F/loader.rs:26`).~~ Fixed.
4. ~~No GIF frame player.~~ Fixed.
5. ~~Animated AVIF doesn't animate.~~ Fixed on macOS through ImageIO; WebP and APNG everywhere.
6. Video and audio hover-play in the grid is built; the hover rows stay Partial because the sound has never been heard by a person and the video strip is a set of pictures, not the file playing.
7. ~~**No video frame tools (Missing).** You can't set a frame as the thumbnail, copy it or save it.~~ Fixed.
8. ~~No fallback player for mkv, avi, wmv, flv or webm.~~ Fixed when FFmpeg is built (`scripts/build-ffmpeg.sh`); AV1 through dav1d.
9. ~~No interactive 3D model viewer.~~ Fixed for obj/stl/ply/glb/3mf/fbx/dae/3ds and ifc with full geometry through web-ifc. Textures are drawn for glb, obj, fbx, dae and 3ds, with the normal, bump and emissive maps and transparency Eagle's viewer draws.
10. ~~**No text editor for .txt (Missing).** No editing, renaming, zoom or auto-reload, and cards don't show the text.~~ Fixed.
11. ~~**No viewer context menu or right-drag gestures (Missing).** Right-click menu, swipe to previous or next, drag to zoom.~~ Fixed.
12. ~~**No zoom slider or zoom presets menu, and different zoom keys (Partial).** Cmd+=/Cmd+-/Cmd+0/Cmd+9 and backtick don't work in the viewer.~~ Fixed.
13. ~~**No presentation mode or full screen (Missing).** Also affects the video full-screen button and PDF presentation mode.~~ Fixed.
14. The separate preview window is built (opacity, always on top, reveal in Finder). Partial: rotate and flip write the file by default as Eagle does, a video's picture turns in the view only, grayscale is per window, and zoom uses the main viewer's sharp renders. None of this has been seen on screen yet, the app menu's View ▸ Grayscale acts on the main window only, and the web and 3D panes are not grayed.
15. ~~**No viewer background options (Missing).** No per-item background, transparency grid preference or pixelated zoom.~~ Fixed.
16. ~~**No navigator minimap or grayscale preview (Missing).**~~ Fixed; each preview window has its own grayscale flag (see the Navigator row).
17. ~~Viewer preferences are absent.~~ Fixed: wheel behaviour, default size, pixelated zoom, remembered zoom, transparency grid, GIF player and grid autoplay.
18. ~~**Remember last zoom and position per item, and persist video position across launches (Missing / Partial).**~~ Fixed.
19. ~~PDF gaps.~~ Fixed: page rotation, hand tool, scroll and spread modes, presentation, print, document properties, password prompt, links, attachments, layers (switched through an in-memory optional-content update PDFKit draws, as a tree, and printed), current outline item, and whole-word and diacritics search are in. The reader is still macOS only (PDFKit).
20. ~~**Crop gaps (Missing).** No width and height fields, no Save As, no size readout and no Shift+C.~~ Fixed.
21. ~~Audio gaps.~~ Fixed: speed control, volume and skip keys are in (see the audio viewer rows).
22. ~~**Font viewer gaps**: fonts egui can't read (some woff2, CFF) still fall back to the image viewer.~~ Fixed.
23. ~~**Space opens the full viewer instead of Eagle's inline quick preview (Partial).**~~ Fixed.
24. ~~**No position counter, prev/next buttons or first/last-item tips in the viewer bar (Missing).**~~ Fixed.
25. ~~**Video gaps (Missing / Partial).** No seek-bar hover thumbnail, flip and rotate, subtitles, volume and seek tip overlay, or hold-to-repeat skip.~~ Fixed.
26. **Design formats rely on QuickLook (Partial).** This works only on macOS and depends on the machine. PSD, PSB, HEIC, TIFF and TGA have their own readers, RAW shows its embedded preview (mrw and raw files with none get no thumbnail), and HDR and EXR are tone mapped with an exposure slider.
27. ~~**No "cannot preview" page with an Open With button, and no "too large" message (Partial).**~~ Fixed.
28. ~~**Long images open fitted to the window instead of to the width (Missing).**~~ Fixed.
29. ~~**No "double-click opens in the default app" preference (Missing).**~~ Fixed.
30. ~~**Fonts egui can't read fall back to the image viewer (Partial).**~~ Fixed.

## Search, filters and organising


Scope: search box, filter bar, sorting, folders, smart folders, tags, Quick Access, inspector, batch editing, duplicates, trash, ratings, comments, saved filters and the other organising commands.

Every falcon status below comes from reading falcon's code, not from its ROADMAP or docs.

Path shorthand:
- Eagle: `B` = `app/app/app.bundle.js`, `R` = `app/app/js/rule-match.js`, `D/` = `app/app/js/directives/`, `en` = `i18n/en.js`, `idx` = `app/app/index.html`.
- Falcon: `S` = `crates/falcon-library/src/search.rs`, `SM` = `.../smart.rs`, `M` = `.../model.rs`, `E` = `.../edit.rs`, `O` = `crates/falcon-app/src/organize.rs`. Other falcon paths are relative to `crates/falcon-app/src/`.

Status values: Done = works as in Eagle. Partial = exists but is missing options or behaves differently. Missing = not there.

### 1. Search box

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Type to search the current view, with a short debounce | Partial | `ui/toolbar/mod.rs` search_field; `logic/scope.rs` visible_in; no debounce, the query applies each frame | local |
| Space between words means AND | Done | `S:115` Query::parse | local |
| `OR` and `\ | \ | ` mean OR | same | Done | `S:115` | local |
| A leading `-word` excludes matches | Done | `S:161` | local |
| Quoted phrases and parentheses, flattened lossily as Eagle does | Done | `S:92` flatten_group | local |
| If the terms fail, the whole query is tried as one substring | Done | `S:174` matches (`raw`) | local |
| Matches item name, `.ext`, URL (queries of 2+ characters), note, tags, and the name and description of each folder the item is in | Done | `S:311` search_text | local |
| Also matches region and video comment text | Done | `search_text_in` with a `SearchScope` (search.rs) | local |
| Also matches camera (`rawMetas.camera`), text-file content (`image.text`) and font metadata | Done | Always searched, as Eagle | local |
| Search Scope menu: turn name, folder name, folder description, extension, tags, URL, comment and note on or off (remembered) | Done | Menu on the search caret, 8 toggles remembered (`search_state.rs`) | local |
| Search All: search the whole library from any view | Done | ⌥⌘F (`find.search.all`) | local |
| Exact-substring results only | Done | Falcon adds typo-tolerant matching, on by default at the user's request (`Prefs::typo_search`); off gives Eagle's exact substrings. A documented falcon extra, not a gap | local |
| Traditional and simplified Chinese match each other | Not wanted (user decision) | Not built by decision (2026-11-01): the user does not want Chinese variant matching in falcon | local |
| Pinyin matching for folder, smart folder and tag names in sidebar and tag lists | Not wanted (user decision) | Not built by decision (2026-11-01): the user does not want pinyin search in falcon | local |
| Toolbar title reads "Search results" while searching | Done | `ui/toolbar/mod.rs` | local |
| Empty-result message | Done | `ui/grid/empty.rs`: per-view messages (No Matches, Drop Files Here, Trash Is Empty, All Sorted…) | local |
| Search by image from the search box (AI) | Missing | The camera icon is drawn disabled (`ui/toolbar/mod.rs` search_field) | online/plugin |

### 2. Filter bar (general)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Show or hide the filter bar | Done | `ui/toolbar/mod.rs`, Command::ToggleFilter | local |
| Badge or highlight while filters are active | Done | `ui/toolbar/mod.rs` outlines the funnel | local |
| Clear all filters | Done | `ui/filter_bar/mod.rs` filter_bar, the Clear button | local |
| Add filter (+) menu: choose and pin which filters appear in the bar | Done | `ui/filter_bar/`: Eagle's default pins, + menu, chips reordered by dragging (`SearchPrefs.order`) | local |
| Lock the filter so it survives switching folders. Unlocked, a view change clears the filter and search | Done | `ui/filter_bar/mod.rs` filter_bar, the Lock button; not kept across launches, as Eagle | local |
| Per-option counts in each filter menu | Done | Tags, folders, type, shape and rating counts (`type_counts`, `shape_counts`); counted over the whole library | local |
| A shortcut for each filter (open the colour, tag, folder... chip) | Done | ⌥F/⌥T/⌥C/⌥S/⌥R/⌥D/⌥E (`find.filter.*`); the others have unbound rows; `find.filter.import` opens the Date chip's Modified tab, `find.filter.other` the + menu (`find.filter.image` is the AI Search plugin's, not built) | local |

### 3. Individual filters

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Colour: preset swatches, any colour, accuracy slider, results sorted by colour distance | Done | `ui/filter_bar/mod.rs` color_chip, `S:870` ColorQuery, `logic/scope.rs` visible_in distance sort | local |
| Colour: grayscale only | Done | `ui/filter_bar/mod.rs` color_chip, `S:922` | local |
| Colour: full picker widget | Done | HSV square and hue strip with a preview beside the hex field (`ui/filter_bar`); falcon's own swatches; Eagle has no recent-colour list either | local |
| Tags: include tags, with Any, All or Equals logic | Done | `ui/filter_bar/mod.rs` tags_chip, `S:362` SetFilter (keeps the OR quirk) | local |
| Tags: Untagged option | Done | `ui/filter_bar/mod.rs` tags_chip | local |
| Tags: exclude a tag (right-click) | Done | `ui/filter_bar/mod.rs` tags_chip | local |
| Tags: search box, Select all, and a sidebar with All, Selected and tag groups; filter by a whole group | Done | Tags chip search, sidebar (All / Selected / groups) and Select all per group | local |
| Folders: include folders with Any, All or Equals; Uncategorized option | Done | `ui/filter_bar/mod.rs` folders_chip | local |
| Folders: exclude, search box, tree view | Done | Folders chip in tree order with counts, search and exclude, Uncategorized included | local |
| Shape: horizontal, vertical, square, panoramic horizontal, panoramic vertical | Done | `ui/filter_bar/media.rs` shape_chip, `S:433` | local |
| Shape: 4:3, 3:4, 16:9, 9:16 and a custom W:H ratio | Done | `Filter::custom_shape`; exact float equality as Eagle; saved-filter keys `43`/`34`/`169`/`916`/`custom` | local |
| Rating: 5 to 1 stars plus None, OR-ed | Done | `ui/filter_bar/mod.rs` rating_chip | local |
| Type: pick extensions | Done | `ui/filter_bar/mod.rs` types_chip | local |
| Type: built-in groups (font, video, audio, YouTube, Vimeo, Bilibili), exclude by right-click, search box, counts | Done | `type_options`, Eagle's include order and quirks (video/audio decide alone); falcon counts font/video/audio so they appear, which Eagle's zero counts never do | local |
| Date Added: Today, Yesterday, last 7/30/90/365 days, custom range, months | Done | `ui/filter_bar/media.rs` media_chip (Date) | local |
| Date Modified: same options | Done | `ui/filter_bar/media.rs` media_chip (Date, Modified tab). Falcon also has a Created tab, which Eagle lacks. | local |
| File size min/max in KB or MB | Done | Size chip | local |
| Dimensions: min/max width and height | Done | Dimensions chip | local |
| Duration min/max with s, m or h | Done | `ui/filter_bar/media.rs` media_chip (Duration) | local |
| BPM min/max | Done | `ui/filter_bar/media.rs` media_chip (BPM) | local |
| Note: has, none, or keywords | Done | `ui/filter_bar/mod.rs` chip (Note) with text_filter_menu | local |
| URL: has, none, or keywords | Done | `ui/filter_bar/mod.rs` chip (URL) with text_filter_menu | local |
| Comment (region and video comments): has, none, or keywords | Done | `Filter::comments`; saved under the `annotation` key | local |
| Camera: pick from the cameras present | Done | `ui/filter_bar/media.rs` media_chip (Camera) | local |
| Font: activated or deactivated | Done | `ui/filter_bar/media.rs` media_chip (Font) | local |
| Visual Match (find by image, drop or paste an image) | Missing | AI search service | online/plugin |
| Semantic search ("black cat") | Missing | AI search service | online/plugin |

### 4. Sorting

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Global sort by Date Added, Created, Modified, Title, Extension, File Size, Dimensions, Rating, Tags, with ascending/descending | Done | `S:1012` OrderBy, `S:1099` sort, `ui/toolbar/layout_panel.rs` panel_sort | local |
| Global sort by Duration and Random | Done | Global sort menu; Duration on whole seconds with missing counted as 0, as Eagle | local |
| A folder or smart folder keeps its own sort, or follows Global Settings | Done | `ui/organize.rs` sort_entries, own_sort_menu, `O:280` finish_order | local |
| Manual sort by dragging items inside a folder | Done | `O:309` manual_sortable, `S:1122` sort_manual, OrgAction::MoveManual | local |
| Natural (numeric-aware) title order | Done | `S:1060` natural_cmp | local |
| Trash sorted by deletion time | Done | `logic/scope.rs` visible_in | local |

### 5. Sidebar views and Quick Access

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| All, Uncategorized, Untagged, All Tags and Trash, with counts | Done | `ui/sidebar/mod.rs` nav_rows, `logic/scope.rs` nav_counts | local |
| Uncategorized also counts items whose only folder no longer exists | Done | `logic::scope::uncategorized` (the view): no folders, or exactly one id that names no folder. `counts_as_uncategorized` (the sidebar count) adds the one-stale-id case only when the item's last-modified time is before the library's `modificationTime`, as Eagle's count does; Eagle's view has no such guard | local |
| Random view | Done | `logic/scope.rs` Scope::Random | local |
| Random refresh button (key R) | Done | Button, R and the View menu | local |
| Recently Used: items the user recently opened | Done | Opened items, newest first; trashed items listed too, as Eagle | local |
| Filter sidebar items | Done | Folders and smart folders filtered, Quick Access not (as Eagle); headers with no match hide; ⌥⇧F focuses the field | local |
| Go to Folder: quick-search folders, smart folders and tags, with history | Done | `ui/dialogs/quick_switch.rs` dialog with history; also an Items tab as Eagle 4 | local |
| Quick Access: add or remove a folder or smart folder | Done | `ui/sidebar/mod.rs` folder_row, `ui/organize.rs` smart_row, `E:595` | local |
| Quick Access: reorder by dragging, drag a folder in to add it | Done | Entries keep their whole JSON object when moved | local |
| Quick Access: drop items on a row to add them to that folder | Done | Drop target rows | local |
| Expand/collapse a folder, all folders, or the same level | Done | Expand/collapse entries in the folder and smart folder menus; ⌘, ⌥ and ⌘⌥ clicks on carets | local |
| Double-click on a sidebar row renames or collapses, per preference | Done | `Prefs::sidebar_double_click` for folder and smart folder rows; a double-click on the icon or count badge always toggles expand, as Eagle's `index.html` (`ui/sidebar/mod.rs` `double_click_toggles`) | local |

### 6. Folders

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| New folder (sidebar +, shortcut) | Done | `ui/sidebar/mod.rs` (sidebar +), Command::NewFolder | local |
| New subfolder | Done | `ui/sidebar/mod.rs` folder_row | local |
| New Folder Below (sibling) | Done | `Action::CreateSibling` | local |
| A new child or sibling inherits the parent's icon and colour | Done | `edit.rs` `create_folder` copies `icon` and `iconColor` from the parent | local |
| Rename (inline) | Done | `ui/sidebar/mod.rs` folder_row | local |
| Batch rename selected folders | Done | `Library::rename_folders` with Eagle's templates (`crates/falcon-library/src/rename.rs`), preview dialog, asks first from 10 folders | local |
| Delete a folder and its subfolders | Done | `ui/sidebar/folders.rs` ask_delete_folders, `E:507` | local |
| Delete-folder checkbox "Move items within folders to Trash too" (on by default, can be unticked) | Done | `Library::delete_folders(keep_items)`; the box is ticked by default; a delete covering 50 or more items asks again, as Eagle | local |
| Delete several selected folders at once | Done | `Library::delete_folders`, one undo step; Quick Access entries removed | local |
| Move a folder by dragging (above, below, into) | Done | `ui/organize.rs` folder_drop, `O:135` tree_drop | local |
| Confirmation dialog when dropping a folder | Done | Eagle asks nothing on a drag either; only its Move Folder dialog confirms, which falcon's does | local |
| Move Folder dialog with destination search | Done | `folders_ui` `move_dialog`; Eagle's doesn't create a folder from the search either | local |
| Move Up, Down, To Top, To Bottom | Done | Edit ▸ Move Folder ▸ Up / Down / To the Top / To the Bottom and ⌘] ⌘[ ⇧⌘] ⇧⌘[ (`Action::Arrange`); not in the folder context menu | local |
| Sort folders by title for this level or all levels, A-Z or Z-A, with confirmation | Done | `ui/organize.rs` folder_title_sort_menu, `O:352`, `ui/organize.rs` organize_dialogs confirm | local |
| Duplicate a folder | Done | `Library::duplicate_folder` | local |
| Folder icon from the named icon set | Done | `ui/organize.rs` icon_grid, `folder_icons.rs:171` | local |
| Folder icon as an emoji | Partial | Falcon addition (Eagle 4 stores no emoji icons; its "emoji" submenu is the named-icon grid): 112 curated emoji in an Emoji submenu for folders and smart folders, drawn in sidebar and Quick Access rows from the system font's bitmaps; kept in falcon's app data, not metadata.json, so Eagle keeps the named icon. No subfolder inheritance, Duplicate or multi-select Emoji menu. Not yet seen on screen | local |
| Folder colour (8 colours or none), also tinting the guide line | Done | `ui/organize.rs` color_row, `ui/sidebar/mod.rs` folder_row guide line | local |
| Change icon or colour of several selected folders | Done | `Library::set_folders_icon` from the multi-folder menu | local |
| Folder cover: set an item as cover | Done | `image_edit.rs:92`, `E:465` | local |
| Folder cover: falls back to the first item, then to subfolders | Done | `covers.rs:44` | local |
| Reset a folder cover | Partial | Falcon addition (Eagle has no reset-cover command): Reset Cover in the sidebar folder menu and subfolder card menu, shown only with a custom cover. Not yet seen on screen | local |
| Folder password: create with hint, change, remove, Lock Now | Done | `ui/organize.rs` folder_password_menu, password_dialog, `O:202` | local |
| Locked folder hides its items and subfolders until unlocked; unlock screen shows the hint | Done | `O:160` hidden_folders, `ui/organize.rs` unlock_screen | local |
| Unlock a folder with Touch ID | Done | Touch ID button on the locked-folder screen whenever the Mac supports it, regardless of the preference, as Eagle; only the asked folder unlocks | local |
| Folder description, edited in the inspector | Done | Saves 1 s after typing stops and on blur, one undo step (`Action::SetFolderDescription`) | local |
| Rename a folder from the inspector name field | Done | `scope_inspector`: Return or blur saves through `Action::RenameFolder`; an emptied name reverts | local |
| Folder auto-tagging: tags added to every item put in the folder, including the parent folders' tags | Done | `crates/falcon-library/src/autotag.rs` (own tags then each parent's, deduped, in Eagle's order); applied on import, paste, Add to Folder, the inspector picker and drops. `extendTags` is derived, as Eagle does, not stored | local |
| Folder settings dialog (name plus auto-tags), Cmd+Shift+R | Done | `ui/organize.rs` auto_tag_dialog; `shortcuts.rs` `edit.folder.setting`; saving appends the tags to the folder's items, as Eagle does | local |
| Show Subfolder Content (folder view includes descendants' items) | Done | `logic::folder_members`, ⌥⌘7 | local |
| Subfolder strip with covers at the top of a folder | Done | `covers.rs:65` subfolder_strip | local |
| Folder item counts | Done | `logic/folders.rs` folder_counts | local |
| Folder inspector: item count, size, date imported, password state | Done | `ui/inspector/mod.rs` scope_inspector | local |
| Folder inspector: set, change or remove password buttons | Done | Create, or Change | Remove, for a single folder; count and size hidden while locked | local |
| Copy Folder Link (eagle:// link) | Done | Localhost URL; `eagle://` under Copy ▸ Eagle Link | local |
| Export folder (computer, .eaglepack) | Done | Folder menu Export ▸ … and the inspector's Export button (`ui/dialogs/export.rs` `tree_export_menu`, `scope_export_button`) | local |
| Clear Invalid Content (empty or broken folders) | Done | Same tool from the library switcher | local |

### 7. Smart folders

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Create a smart folder with a name and rules | Done | `ui/dialogs/smart.rs` smart_dialog_ui, `SM:297` | local |
| Edit rules, rename, delete (with confirmation text) | Done | `ui/organize.rs` smart_row, `ui/dialogs/smart.rs`, `SM:354`/`366`/`373` | local |
| Nested smart folders: a child narrows its parent's rules | Done | `SM:77` contains | local |
| Smart folder groups (no rules, show the union of the children) | Done | `SM:58` is_group, `ui/organize.rs` smart_row | local |
| Drag to reorder and nest | Done | `SM:384`, OrgAction::MoveSmart | local |
| Icon and colour | Done | `ui/organize.rs` smart_row, `SM:417` | local |
| Smart folder's own sort | Done | `ui/organize.rs` smart_row, `SM:429` | local |
| Add to Quick Access | Done | `ui/organize.rs` smart_row | local |
| Duplicate a smart folder | Done | `smart::duplicate_smart_folder` | local |
| Description | Done | Edited in the inspector when a smart folder is open and nothing is selected (Eagle edits it there too, not in the rules dialog) | local |
| Several conditions, each with its own Any/All and true/false (NOT), AND-ed together | Done | `smart_draft.rs` drafts/conditions and `ui/organize.rs` `smart_conditions_ui`: every condition is edited, each with must/must-not and all/any | local |
| Editing a smart folder keeps its other conditions and rules | Done | Untouched conditions and rules are written back verbatim, edited ones keep their unknown keys, and rules the dialog can't show stay as read-only Unsupported rows until deleted (`smart_draft.rs`, test `eagle_smart_folder_survives_an_edit`) | local |
| Live "N items found" count while editing rules | Done | `smart_count.rs` / `smart::count_draft` on a worker, debounced 250 ms, with the parent's conditions | local |
| Batch rename, recolour or delete several smart folders | Done | Multi-smart-folder menu in `folders_ui`, looping the `smart.rs` calls in one undo step | local |
| Export a smart folder | Done | Smart folder menu Export ▸ directory or `.eaglepack` (`export::plan_smart`, `PackSource::Smart`) | local |
| Rule: name, URL, note, with equals, contains, excludes, begins with, ends with, empty, not empty | Done | `smart.rs` text rules; every method is in the dialog for each text property. Begins/ends-with are anchored regexes as in Eagle | local |
| Rule: regex method | Partial | `smart.rs` uses the `regex` crate, lower-casing as Eagle; no look-around or backreferences (documented difference) | local |
| Rule: folder name | Done | `SM:111`, `crates/falcon-library/src/smart_draft.rs` PROPERTIES, `ui/organize.rs` rule_fields_ui | local |
| Rule: folders (in any, all, equal, excludes, empty, not empty) | Done | `smart.rs`; the dialog has a folder checklist (`ui/organize.rs` `rule_fields_ui`) | local |
| Rule: tags (any, all, equal, excludes, empty, not empty) | Done | `SM:124`, `crates/falcon-library/src/smart_draft.rs` PROPERTIES, `ui/organize.rs` rule_fields_ui (typed list, no tag picker) | local |
| Rule: type (is, is not; extensions and groups) | Done | `SM:126` | local |
| Rule: rating (is, is not, contains) | Done | `smart.rs` rating, including "contains" over the stars as text; all three in the dialog | local |
| Rule: shape including custom W:H | Done | `smart.rs` shape; the dialog has custom W:H fields | local |
| Rule: colour (similar, almost the same, grayscale) | Done | `SM:157`, `SM:168` | local |
| Rule: width | Done | `SM:113`, `crates/falcon-library/src/smart_draft.rs` PROPERTIES, `ui/organize.rs` rule_fields_ui | local |
| Rule: height | Done | `smart.rs`; in the dialog | local |
| Rule: file size with KB or MB unit | Done | `smart.rs`; the dialog offers KB/MB and =, <, >, between | local |
| Rule: BPM | Done | `smart.rs`; in the dialog | local |
| Rule: Date Added (on, before, after, between, within last N days) | Done | `smart.rs` compares local calendar days for "on"; the dialog has a calendar picker (`ui/date_picker.rs`) and "within N days" | local |
| Rule: Date Created and Date Modified | Done | `smart.rs`; in the dialog | local |
| Rule: comments | Done | `smart.rs` rule arm over the joined comment texts, with tests | local |
| Rule: camera | Done | `smart.rs` text methods on the lower-cased camera; a photo without one fails every method, as in Eagle | local |
| Rule: ISO, aperture, focal length, shutter | Done | `smart.rs` numeric arms parsed from `rawMetas` as Eagle does (shutter takes the denominator) | local |
| Rule: video duration with h/min/sec | Done | `smart.rs` duration arm with the h/m/s unit | local |
| Rule: timestamp (date taken) | Done | `smart.rs` as a date rule | local |
| Rule: font installed or not | Done | `smart.rs` checks `~/Library/Fonts/EagleApp` for the `<postScriptName>_.<ext>` file, as Eagle does; fonts installed any other way count as not installed | local |

### 8. Tags

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| All Tags page: every tag with its count, filter box, A-Z sections | Done | `ui/grid/tags_page.rs` tag_page | local |
| Click a tag to show its items | Done | `ui/grid/tags_page.rs` tag_chip, Scope::Tag | local |
| Rename a tag everywhere (items, history, starred, groups) | Done | `E:131` plus folder auto-tags and smart-folder tag rules rewritten in place in one undo step (`tag_renames_reach_folder_auto_tags_and_smart_rules_in_one_undo_step`) | local |
| Merge tags: select several and merge them into one name | Done | Merge dialog (2–5 tags as Eagle), reaching folder auto-tags and smart-folder rules | local |
| Delete a tag (with confirmation) | Done | `ui/menus/tags.rs` tags_menu, `E:169` | local |
| Star and unstar tags; Starred section | Done | `ui/menus/tags.rs` tags_menu, `E:187`, `ui/grid/tags_page.rs` tag_list | local |
| Select several tags (click, Shift, Cmd, rubber band) and act on all | Done | Click, ⌘, ⇧ and a rubber band with edge auto-scroll (`tag_view.rs`); the tag menu acts on all | local |
| Tag groups: create, rename, colour, description, reorder, delete | Done | `ui/organize.rs` tag_group_header, tag_group_context_menu, `E:214`-`330` | local |
| Move a tag to a group, remove from group, drag a tag onto a group | Done | `ui/organize.rs` tag_group_menu, DragTag | local |
| Add a tag to more than one group | Done | Add to Group keeps the tag's other groups (`add_tags_to_group_too`) | local |
| Find items with any tag in a group | Done | Group heading menu | local |
| Tag sidebar: All, Starred, Uncategorized (tags in no group), Groups | Done | `ui/grid/tag_sidebar.rs`: All / Uncategorized / Starred / Groups with +, rename, reorder and the group menu; fixed 190 px, not resizable as Eagle's | local |
| Sort tags by name or count, group by alphabet or groups | Done | All Tags page sort menu | local |
| Tag group colour dot on chips | Done | `ui/organize.rs` tag_dot | local |
| Recent tag history (last 120 used) kept in tags.json | Done | `tagging.rs` `add_tags_to_items` and `push_history`: each added tag moves to the front, 120 kept, a tag removed from its last item leaves the history | local |
| Clear tag history | Done | Button on the All Tags page, `Library::clear_tag_history` | local |

### 9. Inspector (one item)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Preview; double-click to open | Done | `ui/inspector/mod.rs` item_inspector | local |
| Colour palette swatches | Done | `ui/inspector/mod.rs` item_inspector, palette | local |
| Right-click a swatch to find items of similar colour | Done | `ui/inspector/mod.rs` item_inspector, swatch_menu | local |
| Copy the colour as HEX, RGB or RGBA | Done | Swatch left click opens the menu: HEX, RGB, RGBA, HSL, HSV, HWB, CMYK (`colors.rs`); right click filters by the colour, as Eagle's `openColorContextMenu` and `filterWithColor` | local |
| Set a palette colour as the primary colour (`marked`) | Done | Stored as Eagle writes it; colour search matches a marked colour under ΔE2000 30 as Eagle; smart-folder colour rules use Eagle's stricter test without it, as Eagle | local |
| Re-analyse colours | Done | `image_edit.rs:107` Refresh Palette | local |
| Name (title) editing | Done | `ui/inspector/mod.rs` item_inspector | local |
| Click the extension to change it | Done | Inspector Type row opens a checking dialog; only the original is renamed, as Eagle | local |
| Note editing | Done | `ui/inspector/mod.rs` item_inspector | local |
| URL editing, validated as Eagle does | Done | `urls.rs` `url_accepted`: web URLs and existing paths, as Eagle; Open Link icon beside the field | local |
| Open Link in Browser button | Done | Open Link button beside the URL field, View ▸ Open Link in Browser, ⇧⌘O | local |
| Tags: chips, remove, type a new tag | Done | `tag_field`: chips with × and a + that opens the picker, where typing happens, as Eagle | local |
| Tag picker with Selected, Starred, Recent, Recommended and groups, search, column sizes | Done | `app/tag_picker.rs`, `tags.rs`: search and ranking, Starred, Recent (latest 12 in use), Recommended, groups with a Tab sidebar, settings; no Pinyin or jieba search, no dragging or resizing | local |
| Copy, paste and clear an item's tags | Done | `Library::paste_tags`, `clear_tags`; one undo step and a notice | local |
| Folders: chips, remove, Add Category | Done | `folder_chip` in `ui/widgets.rs`, used by `ui/inspector/mod.rs`; removal is the × on the chip | local |
| Click a folder chip to open that folder | Done | `folder_chip` in `ui/widgets.rs`, used by `ui/inspector/mod.rs`: the click opens the folder with the item still selected and scrolled to; the tooltip shows the folder path | local |
| Folder picker with search, Recent, Selected and create-new | Partial | `add_to_folders.rs`, `folder_picker.rs`; All / Recent / Selected tabs in Eagle's order (Tab cycles; All default), search, Create “…” and applied on close are built and the list rules tested. Gaps: not yet seen on screen; search is substring, not fuzzy / pinyin; the Recent row's remove is a hover × (Eagle: right-click ▸ Remove); no key-hint footer | local |
| Comments list: jump to, edit, delete | Done | `comments.rs:427` | local |
| Rating stars; click the same star again to clear | Done | `rating` in `ui/widgets.rs`, used by `ui/inspector/mod.rs` | local |
| Dimensions, size, type, date imported, created, modified | Done | `ui/inspector/mod.rs` item_inspector | local |
| Duration and BPM | Done | `ui/viewer/audio.rs` media_props | local |
| EXIF: camera, ISO, aperture, focal length, shutter, timestamp | Done | `ui/viewer/audio.rs` media_props | local |
| Collapse or expand inspector sections | Done | `fold_heading` on Properties, Tags, Folders in every inspector and Comments in the viewer | local |

### 10. Batch editing (several items selected)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Preview stack and "N items selected" | Done | `ui/inspector/mod.rs` multi_inspector | local |
| Add a tag to all; remove a tag they share | Done | The picker and the chip × for several items | local |
| Add all to a folder | Done | `ui/inspector/mod.rs` multi_inspector | local |
| Show the folders they share, remove all from one | Done | `shared_folders` chips with × in the multi-item inspector | local |
| Edit the name, note or URL of all at once ("Multiple titles", "multiple URLs") | Done | `set_notes`, `set_urls` from the multi-item inspector; Eagle has no batch name either | local |
| Rate all | Done | `ui/inspector/mod.rs` multi_inspector, Action::BatchRating | local |
| Total size | Done | `ui/inspector/mod.rs` multi_inspector | local |
| Batch rename with a template (%NN, $D, original name, tags, folders, index, date) and find/replace with regex | Done | Rename N Items dialog (`rename.rs`): `%NN`, dates, original name, tags, folders, `%F`, index, find/replace | local |
| Confirm bulk actions on many items | Done | `push_bulk` / `push_bulk_at`: rating, trash, drops, batch notes/URL, the folder panel, Add to Last Used, Pin/Unpin, Show in Finder and Open in default app on several, Remove from Group; folder delete at 50 items; action runs ask from 30 items (`RUN_CONFIRM`, `push_bulk_at`), as Eagle's Actions menu does. Eagle also gates fonts and plugins (out of scope) | local |
| New Folder with Selection | Done | Item menu, `folders_ui` `new_with_dialog` | local |
| Add To Last Used Folder | Done | ⇧D, Organize menu, card menu | local |
| Remove from the current folder | Done | ⇧⌘⌫, Edit menu and card menu | local |
| Pin items to the top of a folder | Done | `item_ops.rs` pin/unpin | local |
| Open File Location (show the item in All or in one of its folders) | Done | Card menu | local |
| Rating shortcuts (keys 0-5) | Done | `organize.rating.*` keys 0–5 with a toast | local |
| Duplicate item | Done | ⌘D, `item_ops.rs` | local |

### 11. Duplicates

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Find identical files | Done | `ui/grid/duplicates.rs` scan, `duplicates.rs:91` content hash | local |
| Find similar-looking images, with a similarity slider | Done | `duplicates.rs:173` perceptual hash, `ui/grid/duplicates.rs` scan_controls slider | local |
| Choose the method (same or similar) before scanning | Done | Duplicates page: method, range, similarity | local |
| Scan all, the current list, or the selected items | Done | `merge.rs` scan ranges | local |
| Progress while scanning | Done | `ui/grid/duplicates.rs` duplicates_page progress bar | local |
| Best copy picked automatically for similar groups (highest quality) | Done | Most pixels, then bigger file of the same format, else Eagle's format order; the kept copy can be changed | local |
| Remove an item or a group from the results | Done | Duplicates page | local |
| Merge: keep one file and combine the others' folders, tags, link and notes into it, choosing the file name | Done | `crates/falcon-library/src/merge.rs`: the kept item takes the folders, tags, notes, link and highest rating; the others are trashed, nothing deleted; one undo step; always asks first | local |
| Summary of files deleted and space saved | Done | After the merge | local |
| Move duplicates to Trash | Done | `ui/grid/duplicates.rs` merge_review | local |

### 12. Trash and ratings

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Move to Trash (menu, key, drag onto Trash) | Done | `ui/menus/item.rs` item_menu_body, `ui/sidebar/mod.rs` nav_rows | local |
| When an item is in several folders, ask whether to remove it from this folder or trash it | Done | Trashing inside a folder asks "only this folder" or "trash all"; the answer is kept for the session | local |
| Restore selected items | Done | `ui/menus/item.rs` item_menu_body, `E:94` | local |
| Restore All Items (with confirmation) | Done | Trash menu | local |
| Delete permanently (with confirmation) | Done | `ui/menus/item.rs` item_menu_body, `E:108` | local |
| Empty Trash (with confirmation) | Done | `ui/sidebar/mod.rs` nav_rows, `E:122` | local |
| Ratings stored as 1-5, key removed when cleared | Done | `ui/inspector/mod.rs` item_inspector, `app/actions.rs` BatchRating | local |
| Toast "rated as N star" | Done | "Rated N stars" / "Removed the rating" notice from the rating keys and menu | local |

### 13. Comments (annotations)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Region comments on images: draw, move, resize, edit, delete | Done | `comments.rs` | local |
| Timed comments on videos | Done | `ui/viewer/video.rs` timed, timed_comment_editor | local |
| Filter by comments | Done | Comment filter chip (`FilterKind::Comment`), Find menu, ⌥-key | local |
| Search comment text | Done | `search_text_in` includes comment texts when the Comments scope is on (default on) | local |
| Smart folder rule on comments | Done | `smart.rs` `comments_rule`, offered in the dialog | local |

### 14. Saved filters

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Save the current filter (and search) under a name | Done | `ui/organize.rs` filter_name_dialog, `E:334` | local |
| Apply, rename, remove, reorder saved filters | Done | `ui/organize.rs` saved_filters_menu, apply_saved_filter | local |
| Saving keeps every active rule | Done | `SavedFilter::rule_from_filter` in model.rs writes dates (Eagle's shared `import.range`), duration (`min`/`max`/`unit`), camera and font in Eagle's key names; Date Created goes under falcon's own `btime` key | local |
| Applying restores every saved rule | Done | `SavedFilter::to_filter` restores every rule; only Eagle's AI rules (`image`, `semantic`) are skipped with a warning | local |
| Saved filters button appears in the filter bar | Done | `ui/organize.rs` saved_filters_menu, drawn by `ui/filter_bar/mod.rs` filter_bar | local |

### 15. Cross-references (other agents' areas)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Undo for organising edits with an UNDO toast | Done | ⌘Z through transactions plus the toast's Undo button | local |
| Layout "Show annotation" on cards | n/a | main window agent | local |
| Context-menu search box | n/a | shell agent | local |

### Ranked list of local Missing and Partial items

Ranked by user impact. Data-loss and silent-mismatch bugs come first, then everyday organising, then niche items.

1. ~~Editing a smart folder loses its rules.~~ Fixed.
2. ~~Folder auto-tagging is ignored.~~ Fixed: applied everywhere items join a folder, with a settings dialog.
3. ~~Smart folder rules always fail for comments, camera, ISO, aperture, focal length, shutter, duration, timestamp and font.~~ Fixed, with real regex and local-day "on".
4. ~~**Saved filters drop and skip rules.** Saving drops date, duration, camera and font. Applying skips them, plus comments, ratio shapes and excluding Uncategorized.~~ Fixed.
5. ~~Folder description and inspector rename don't save.~~ Fixed.
6. ~~Clicking a folder chip in the inspector removes the item from the folder.~~ Fixed: it opens the folder; the chip's × removes.
7. ~~**No size or dimensions filter chips.** The engine supports them, but there's no UI.~~ Fixed.
8. ~~**No Comment filter, and comments, camera, text content and fonts aren't searched.** There's also no Search Scope menu.~~ Fixed; the search-by-image camera stays disabled (online).
9. ~~The smart folder dialog has no date picker and shows no live hit count.~~ Fixed.
10. ~~No tag picker.~~ Fixed.
11. ~~**No exclude, search or tag-group options in the Tag, Folder and Type filters.** Also no type groups (video, audio, font...).~~ Fixed.
12. ~~Remove from current folder is missing, and the "remove from folder or trash" prompt.~~ Fixed: ⇧⌘⌫, and trashing in a folder asks "only this folder" or "trash all" when items are filed elsewhere too.
13. ~~No Show Subfolder Content toggle.~~ Fixed.
14. ~~**Batch rename of items is missing**, and so is batch name/note/URL editing for several items (folders can be batch-renamed).~~ Fixed.
15. ~~**Duplicates can't merge.** Folders, tags, notes and links aren't combined; there's no best-copy pick, no scope (current list or selected), and no remove-from-results.~~ Fixed.
16. ~~No Go to Folder quick search.~~ Fixed.
17. ~~No multi-select of folders.~~ Fixed, for folders and smart folders.
18. ~~Missing folder commands.~~ Fixed, including Add to Last Used Folder.
19. Eagle 4 stores no emoji icons (its "emoji" submenu is the named-icon grid) and has no reset-cover command, so Eagle has neither; falcon now has both as additions (Partial until seen on screen). A new folder inherits the parent's icon and colour.
20. ~~No Pin to Top for items, and no Open File Location.~~ Fixed.
21. ~~**Tag manager gaps**: no Uncategorized or Starred tag pages, no rubber-band tag selection, and rename or merge don't reach folder auto-tags or smart-folder rules.~~ Fixed.
22. ~~Quick Access can't be reordered, dragged into, or used as a drop target.~~ Fixed.
23. ~~**Filter bar gaps**: no Filter Lock toggle (falcon always keeps the filter and search when switching views), no add or pin filters, no ratio shapes (4:3, 16:9, custom), no counts for type, shape and rating.~~ Fixed.
24. ~~Global sort lacks Duration and Random.~~ Fixed.
25. ~~**Rating gaps**: no rating shortcuts (0-5) and no rating toast.~~ Fixed.
26. ~~Palette gaps.~~ Fixed: colour search matches a marked primary colour under ΔE2000 30, as Eagle.
27. ~~No change-extension in the inspector; URL isn't validated; no Open Link button.~~ Fixed.
28. ~~Recently Used isn't a list of opened items; Uncategorized ignores stale folder ids.~~ Fixed (R reshuffles Random).
29. ~~**Folder deletion**: the trash checkbox can now be unticked; there's no move confirmation, and Touch ID unlock is missing.~~ Fixed.
30. **Search extras**: Search All is done. Chinese traditional/simplified matching and pinyin sidebar search are not wanted (user decision). Falcon's own fuzzy matching is a documented falcon extra, on by default with a switch.
31. ~~**Minor UI gaps**: Smart Folders and Folders section headers don't fold; the bulk-action gate doesn't cover every newer command yet.~~ Fixed.

Online/plugin only (not ranked): Visual Match, Semantic search, the search-by-image button.

## Import, export, library, preferences and integrations


Scope: what users can do in this area. Eagle paths are relative to `eagle-src/app/`. "B:" means `app/app.bundle.js`, "BG:" means `app/js/background.js`, "PH:" means `app/preferences.html` (the `ng-model` list), "DP:" means `app/js/default-preferences.js`, and "en:" means a key in `i18n/en.js`. Falcon paths are relative to the repo root. Every falcon claim below was checked in code, not in the docs or ROADMAP.

Status meanings: **Done** means it works in the falcon app, or the API for API rows. **Partial** means it exists only in the CLI or library crate, or behaves differently. **Missing** means it does not exist.

Two cross-cutting facts explain many rows:
- Falcon's native menu bar (`menu.rs`) carries File, Edit, Find, Organize, View, Window and Help; Eagle's online and plugin entries are left out.
- Falcon's `Prefs` struct (`crates/falcon-app/src/prefs.rs`) holds the app's settings, and the Preferences window has ten panes: General, Sidebar, Controls, Preview, Screenshot, Shortcuts, Notification, Password, Auto-Import and Developer. §6 lists every Eagle setting and the few falcon has no switch for.

---

### 1. Import sources

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Drag files from Finder onto the main window to import them into the current folder | Done | `crates/falcon-app/src/app/mod.rs` (`dropped_files` are handed to `drop_files`); `app/import.rs` `drop_files` sends them to `import`, and the folder comes from `scope_folders` in `app/import.rs` | local |
| Drag a folder onto the sidebar to import it with its subfolder tree recreated as Eagle folders | Done | `crates/falcon-library/src/folder_import.rs` (walk, plan, `create_folders`); `app/import.rs` `import_folder_trees` | local |
| Import Folders from a file dialog (several folders, structure kept) | Done | `app/import.rs` `run_import_request`; Import Folders… and Import Files… in the library switcher and on the empty All and folder pages | local |
| Warning before importing a very large folder ("contains N items, continue?") | Done | `folder_import::verdict`: 2,000 files or more ask first, as Eagle | local |
| Refuse folders deeper than 15 levels or with 100k+ files | Done | `folder_import::verdict`, Eagle's numbers | local |
| Dropping or picking a `.library` folder opens it as a library instead of importing it | Done | `logic::split_libraries` separates `*.library` paths before the import walk in `import_into`; the first opens through the sandbox-guarded `App::open`, the rest are named in a toast. Like Eagle, a `.library` nested inside a dropped folder is not special-cased | local |
| A Pixave `.pxvlibrary` folder is skipped by folder import | Done | `folder_import.rs` walk (everywhere, not only the dialog) | local |
| Paste files copied in Finder (⌘V) | Done | `app/import.rs` `paste_into` (`arboard` file list) | local |
| Paste a copied bitmap image as a new PNG item | Done | `app/import.rs` `paste_into` (named "Pasted image <date>") | local |
| Paste a URL from the clipboard: an image URL is downloaded, a web page is saved as a bookmark | Missing | `paste_into` handles only files and images | online |
| Import from links: a dialog to paste many URLs, one per line, prefilled from the clipboard | Missing | — | online |
| Web bookmark items (save a page URL as an item, with a capture of the page) | Missing | `v2.rs:301-302` rejects `bookmarkURL` with "bookmarks are not supported" | online |
| Drag an image or link from a web browser into the window | Missing | Only `dropped_files` with local paths are read (`app/mod.rs`) | online |
| Batch-download panel: pick images from a page, filter by type and size, select all or invert | Missing | — | online |
| Screen capture of an area, a window or the full screen into the library | Done | `capture.rs`: area (`screencapture -i`), window (`-w`), full screen (`-D` of the pointer's display); from the menu bar icon, the global keys and `Command::Capture`; Eagle's naming "Screenshot - date", format conversion, @2x halving | local |
| Watched auto-import folder: files dropped into it are imported automatically | Done | `fsevents` stream on the chosen folder (2 s poll as fallback), one watcher per open library, settle rule, delete-after-import option (Trash); any format accepted where Eagle takes only its supported ones | local |
| Auto-import rejects network drives and missing paths | Done | `auto_import::is_network_volume` (`NSURLVolumeIsLocalKey`), `prefs::auto_import_refusal`; a vanished folder pauses with a notice | local |
| Auto-import skips `.tmp` files, tiny files and unsupported types, and waits until a copy has finished | Done | `auto_import::skip` and `Settle` (size and mtime unchanged for 3 s); any extension is accepted, as a drop is | local |
| Import an Eagle Package (`.eaglepack`) from the File menu or by opening the file | Done | File ▸ Import submenu, the switcher, a drop, and Finder Open With / Dock drop through `open_files.rs` and the bundle's document types | local |
| Confirm the disk space an `.eaglepack` will use before importing it | Done | `preview_pack` confirmation | local |
| When an `.eaglepack` holds items already in the library, ask "N repeated files, import anyway?" | Done | Count in the title, Import / Leave Them Out, Esc imports everything, as Eagle | local |
| Merge another Eagle library into the current one (folders and items), refusing the library already open | Done | `crates/falcon-library/src/library_merge.rs`: Eagle's id matching for folders, smart folders and tag groups; item folders copied in the background with progress and Stop; the open library is refused | local |
| Import an Inboard backup folder (`InboardBackup.json`), with its folders recreated | Done | `ImportOptions::trashed` and `added_at`; folders recreated | local |
| Import a Pixave library | Not wanted (user decision) | Eagle 4.0.0 has no Pixave importer (no menu entry, only unused strings); falcon's probe importer was removed in b586862f to match. The `.pxvlibrary` skip in folder import stays | local |
| Import from Billfish | — (not counted) | — | — |
| Import from Pinterest | Missing | — | online |
| Import from Huaban | Missing | — | online |
| Import an ArtStation artist's portfolio by URL into a folder, all at once or picked by hand | Missing | — | online |
| Add files by opening them with the app from Finder (Eagle registers for about 36 image, video and design types, folders and packages) | Partial | `CFBundleDocumentTypes` in `scripts/bundle.sh` (Eagle's 30 types, `.eaglepack`, `.library`, folders) and the open-documents Apple event (`open_files.rs`) importing or opening them; not yet checked in a built bundle | local |
| Add items from another app through the HTTP API (`addFromPath`, `addFromPaths`, `addFromURL`, `addFromURLs`, v2 `item/add`) | Done | `crates/falcon-server/src/lib.rs:369-372,386`; data URLs and http(s) downloads in `add_url` (`lib.rs:753-779`) | local, or online for URLs |
| Add an item from base64 data (v2 `item/add` with `base64`) | Done | `crates/falcon-server/src/v2.rs:303-305` | local |
| Import into a folder chosen in the collect window | Done | `ui/dialogs/collect.rs` `collect_window` (`import_into` / `paste_into` with the chosen folders) | local |

### 2. Import options and behaviour

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Copy the original into `images/<ID>.info`, keep the source mtime, make a thumbnail and palette | Done | `crates/falcon-library/src/import.rs:79-147,173-207` | local |
| Imports run in the background with a progress display | Done | `app/import.rs` `spawn_jobs` (background thread); the progress bar is `progress_bar` in `ui/toolbar/mod.rs` | local |
| Upload queue panel listing failed imports with reasons (download failed, timeout, cannot decode and so on), plus Retry All, Copy to Clipboard and Clear All | Done | `import::Failure` / `FailKind` per file; `app/import.rs` `error_panel` with Retry All, Copy to Clipboard (the files), Clear All; opened from a toast. Local-file reasons only | local |
| "Import Successful, N image(s) imported" notification | Done | In-window note plus a system notification from the bundled app | local |
| Duplicate check on import (Eagle's metadata key), with a dialog offering Import, Don't Import, Keep both, Use existing file and Apply to all | Done | `import_check.rs` (same size and extension, then a BLAKE3 hash of the whole file), `crates/falcon-app/src/import_dupes.rs`, `ui/dialogs/duplicates.rs` `duplicate_dialog`: Eagle's two choices (use existing, keep both), Import, close = don't import, apply to all. One thumbnail shown, not two | local |
| Setting: notify me when importing duplicates | Done | `Prefs::notify_duplicates`, on by default; off imports duplicates silently as Eagle does | local |
| Folder auto-tagging: tags set on a folder are added to every item imported or pasted into it | Done | `ImportOptions::folder_auto_tags`, set by `Prefs::import_options`; applied in `commit_import` | local |
| Screenshot auto-tag ("Screenshot" tag) | Done | "Screenshot" tag when the item has no tags, as Eagle (`screenshot.autoTagging`) | local |
| Add IPTC and XMP keywords as tags (setting) | Done | `prefs.rs:24,327`; `import.rs:121-123` | local |
| Font style tag (Regular, Bold, Italic) added on import (setting) | Done | `Prefs::font_style_tags` in the Preview pane, on by default | local |
| Auto-select the new items after import (setting) | Done | `Prefs::auto_select`, on by default; selects the new items shown in the view and scrolls to a single one | local |
| Unsupported formats are refused with an "unsupported format" dialog | Done | `import::is_recognized`: like Eagle, only extensionless files and unfinished browser downloads (`.crdownload`, `.part`, `.download`) are refused, into the error list; every other type imports | local |
| Empty files are refused (except `.txt`) | Done | `import.rs:88-90` | local |
| Hidden files are skipped during folder walks | Done | `app/import.rs` `is_hidden` and `import_files`; `falcon_library::folder_import::walk` | local |
| Not-enough-disk-space check before copying | Done | Eagle has no pre-copy check on import either; ENOSPC is reported per file as "Not enough disk space" in the error list | local |
| Filename sanitising and length limit | Done | `import.rs:411-430` | local |
| Cut mode: files staged in app data (pasted or downloaded) are moved, not copied | Done | Pasted temp files are deleted after import (`app/import.rs` `spawn_jobs`) | local |
| Choose where a screenshot or extension item is saved (collect prompt) | Done | Collect-window prompt for area and window captures (`showCaptureCollectModal`); the browser extension is an online feature with no local counterpart | local |

### 3. Collect window

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| A small floating window that stays on top and takes drops and pastes | Done | `crates/falcon-app/src/collect.rs:83-95` (always on top, borderless) | local |
| Choose the destination folder, with search | Done | `collect.rs:28-54` (flattened tree with search) | local |
| Send to several folders at once | Done | Collect window | local |
| Recent folders list | Done | Collect window | local |
| Create a new folder from the collect window | Done | Collect window | local |
| Set name, description and tags before collecting (tag picker with recent, starred and recommended tags) | Done | Collect window with starred, recent and all tags (no recommended tags); `ImportOptions::name` | local |
| Toggle the collect prompt over the API (`/api/preferences/collect/on`, `/off`) | Done | `preferences/collect/on|off` served; `Handle::set_preferences`; the app keeps the flag | local |
| Window position remembered between launches | Done | `collect.rs:65-67` | local |

### 4. Export and copy-out

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Export to Computer: copy the selected originals to a chosen folder (⌘E) | Done | `crates/falcon-library/src/export.rs` `plan_items` and `copy`; background job with a progress bar; Eagle's `name-1.ext` numbering; reveals the files when done | local |
| Export a folder, subfolder or smart folder to the computer as a directory tree | Done | `export::plan_folder` / `plan_smart`; locked subfolders are left out | local |
| Overwrite prompts when exported files or folders already exist | Done | Replace / Keep Both / Cancel (`ui/dialogs/export.rs` `clash_dialog`); Eagle offers Overwrite or Cancel | local |
| Export selected items as `.eaglepack` (⇧⌘E) | Done | `ui/dialogs/export.rs` `start_pack`; `pack.rs` `plan_pack` / `write_pack` run off the UI thread | local |
| Export a folder (with subfolders) as `.eaglepack` | Done | Folder menu and inspector button | local |
| Export a smart folder as `.eaglepack` | Done | `pack.rs` `PackSource::Smart`: one flat folder, as Eagle 4; CLI `export --smart` | local |
| Export in a chosen format and size (custom export) | Done | ⇧E, Export ▸ Export in Format…: JPG, PNG, WebP, quality, longest side (`export::plan_converted`). Eagle's dialog is a plugin outside its source, so the options are falcon's own | local |
| Export metadata to CSV (ID, name, extension, size, URL, notes, tags, folders, comments and so on) | Done | `export::csv`: Eagle's 16 columns in order, BOM and `\n` line ends, falcon's own headers; item menu and inspector | local |
| "Export Successful, N item(s) exported" notification | Done | In-window note, Finder reveal and a system notification from the bundled app; CSV export posts none | local |
| Drag items out to Finder or other apps | Done | `ui/grid/drag_out.rs` (`drag_image`, `drag::start_drag`), `native_drag` in `app/mod.rs` | local |
| Copy the selected files to the clipboard (⌘C) | Done | `app/import.rs` `copy_items` | local |
| Copy File Path (⌥⌘C) | Done | All selected items, one per line (`ui/dialogs/export.rs` `copy_variant`) | local |
| Copy folder path (⌥⇧⌘C) | Done | The selected items' `<id>.info` folders, as Eagle | local |
| Copy Name | Done | `ui/dialogs/export.rs` `item_copy_menu` | local |
| Copy Thumbnail (as an image) | Done | One item: image on the clipboard; several: the thumbnail files, as Eagle | local |
| Copy Base64 | Done | One item: data URL of the original for jpg/png/webp/gif, else of the thumbnail | local |
| Copy Eagle Link (`http://localhost:<port>/item?id=<id>`) | Done | Copy ▸ Eagle Link in the item menu, Edit ▸ Copy Details, `edit.copy.eaglelink` (unbound, as Eagle) | local |
| Copy Link (the item's source URL) | Done | The source URL has its own entry, Copy ▸ Source URL (`edit.copy.sourceurl`) | local |
| Copy the image to the clipboard as pixels plus file reference (the AppleScript helper) | Done | One jpg/png/bmp: raw bytes, TIFF, `public.file-url` and `NSFilenamesPboardType` together; several items: a file list, as Eagle | local |
| macOS Share sheet for items | Done | Share… in the item menu over the selected files (`share.rs`, `NSSharingServicePicker`) | local |
| Add selected items to another library (from the recent list, or choose a path) | Done | `add_to_library.rs` (both crates): item, single-folder, folder-card, smart-folder and tag-group menus; recent libraries plus Choose Library…; whole `.info` folders copied under a temporary name, existing ids skipped, folder ids kept as Eagle; target must pass the sandbox and install-folder guards; asks above one item; progress with Stop | local |
| Convert WebP items to PNG or JPG (Eagle's "Convert WebP…") | Done | Eagle's tool converts **WebP items to PNG or JPG** (not to WebP): item menu ▸ Convert WebP ▸ PNG/JPG for every WebP in the selection, warning first, animated skipped, old file to the macOS Trash (irreversible for ⌘Z), `ext`/`size` updated | local |
| Save the current video frame as an item or copy it | Partial | Main viewer as Eagle: right-click menu and ⇧S / ⇧C, a JPEG item named after the time, in the video's folders (`ui/viewer/video.rs`). Preview window: copy as Eagle; Save writes a PNG through a file dialog (`ui/viewer/preview_window/mod.rs`), where Eagle's `videoScreenShot` (`js/preview-window.js`) adds a JPEG item to the library | local |

### 5. Library management

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Create a library | Done | `app/library.rs` `new_library` (the create dialog is in `ui/dialogs/library.rs`); `crates/falcon-library/src/library.rs:98-122` | local |
| Create-library dialog with a name field, name validation and folder choice | Done | Eagle's forbidden characters, folder picker | local |
| Warn when creating a library in Desktop, Documents or Downloads | Done | Desktop and Downloads, as Eagle (substring match) | local |
| Refuse a path that already holds a library | Done | `library.rs:101-106` refuses any existing path | local |
| Refuse libraries inside the app's install folder | Done | `install_dir::guard` on create, open, background open, reload, Empty Cache and Add to Other Library | local |
| Open an existing library | Done | `app/library.rs` `pick_library` and `open` | local |
| Reject a folder that is not a library | Done | `library.rs:53-55` (`NotALibrary`), shown in the empty state (`ui/dialogs/library.rs` `empty_state`) | local |
| Switch library (⌘L) through a switcher listing libraries | Done | `app/commands/mod.rs` (`SwitchLibrary`), `ui/sidebar/mod.rs` (the switcher popup) | local |
| Recent libraries history | Done | `crates/falcon-app/src/workspaces.rs:119-129`, `ui/sidebar/mod.rs` (the switcher popup; 10 entries, `RECENT_LIMIT` in `app/library.rs`) | local |
| Clear library history | Done | Switcher menu | local |
| Remove one library from the list | Done | Switcher menu; missing libraries are kept and shown dimmed | local |
| Library list menu: Remove from Disk | Partial | Falcon addition, not an Eagle feature (Eagle's string is the Remove Icon confirmation): moves a closed library inside the sandbox to the macOS Trash after a confirmation naming it, its path and size (`library_removal.rs`, `ui/dialogs/remove_library.rs`). Not yet seen on screen | local |
| Set or remove a library icon (`icon.png`) | Done | `library_icon.rs`: shorter side scaled to 64 px, written atomically; Change Icon / Remove Icon in the switcher | local |
| Warn before switching while tasks run (Force Switch), or block while an AI action runs | Partial | "Stop Tasks and Close" on Close Library and on Quit (exports stop too); falcon's switching keeps tasks running by design, so no switch warning; no AI actions exist | local |
| Several libraries open at once | Done (beyond Eagle) | `workspaces.rs` (several workspaces), `app/library.rs` `add_workspace` and `focus` | local |
| Show the library in Finder | Done | `ui/sidebar/mod.rs` ("Show in Finder") | local |
| Missing-library screen with Try Again, Select another path and Create a new library | Done | At launch, from a missing recent entry, and when an open library vanishes | local |
| Relocate a library that moved | Done | Select Another Path; the old path leaves the history | local |
| Rename a library | — (not counted) | — | — |
| Refuse a library made by a newer Eagle and prompt to update | Done | Version check against 4.0.0 with Eagle's ordering; an alert, since falcon has no updater | local |
| Automatic `metadata.json` backups (on change and every 30 minutes, up to 100 kept) | Done | `crates/falcon-app/src/backups.rs` schedules a backup 1 s after a metadata write and every 30 minutes, run off the UI thread by `run_backups` in `app/actions.rs`; `flush_and_backup` takes a waiting one when a library closes or the app saves state. Written by `backup::backup_library` | local |
| Restore a damaged `metadata.json` from the newest good backup on open | Done | `App::open` and `follow_api_switch` use `Library::open_or_restore`; `restored_note` names the backup used in the toast. The HTTP API's own `library/switch` still uses `Library::open` | local |
| Keep a per-library backup copy in app data (`eagle-backup/<hash>.json`) | Done | `~/Library/Application Support/Falcon/eagle-backup/<hash>.json` with Eagle's FNV-1a path hash, written with every new backup | local |
| Empty cache and reload the library (forced rebuild) | Done | Rebuilds `mtime.json` from the items as Eagle does | local |
| Repair tool "Clear Invalid Content" (removes empty or broken item folders, then reloads) | Done | Switcher ▸ Clear Invalid Content…: background scan, sync warning, orphan `.info` folders to the macOS Trash, items without originals to the library Trash, missing folders dropped, then reload; Eagle hides it behind a modifier-click | local |
| Refresh thumbnails for the selection | Done | `crates/falcon-app/src/image_edit.rs:104-106,151-171` | local |
| Refresh the palette for the selection | Done | `image_edit.rs:107-109` | local |
| Detect external changes to `metadata.json` and `mtime.json` (sync tools) and reload | Done | `sync.rs`, `watch.rs`: polled every 4 s, parsed off the UI thread, nothing written back; a vanished folder closes the library | local |
| Cloud-sync conflict detection (disk copy newer than memory, so warn and reload) | Done | Every metadata write first reads the disk copy and refuses if it is newer (`Error::SyncConflict`); the app alerts and reloads | local |
| iCloud placeholder handling (`.icloud` names, files not yet downloaded) | Done | `crates/falcon-app/src/icloud.rs` (dataless detection, background download, wait screen in `ui/viewer/open.rs` `icloud_viewer`) | local |
| "File not found, sync may be in progress" hint for missing originals | Done | Card tooltip (`broken_thumb_hint`) and the viewer placeholder line | local |
| Library storage health warnings (read-only path, slow disk, NTFS on macOS, permission errors) | Done | Read-only file, no write permission, slow disk (falcon's own 1.5 s round-trip threshold), NTFS volume | local |
| Library loading progress screen | Done | `Library::open_with_progress` | local |
| Library stats (item count, size, trash count) | Done | The scope inspector shows Items and Size for every scope incl. All and Trash, as Eagle's inspector; falcon counts the filtered list | local |
| Welcome or onboarding flow (theme, extension install, create library) | Done | First-launch welcome with theme choice and create or open; no extension step | local |
| Password lock for the whole app: password, hint, Lock Now (⌥⌘L), Touch ID unlock | Done | `applock.rs` with Touch ID unlock (`touch_id.rs`) | local |

### 6. Preferences (every pane and setting)

Eagle panes (`app/js/preferences.js:659-788`): General, Sidebar, Controls, Preview (habits), Screenshot, Shortcuts, Notification, Password, Auto-Import, AI Search, AI Models, Eagle MCP and Developer. The Proxy pane is in the i18n file but not listed in this build. Falcon panes: General, Preview, Shortcuts and Developer.

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Preferences window with Save, Apply and Cancel | Done | `prefs.rs:250-263` | local |
| Search box across preferences | Done | `prefs.rs` `matches` over every pane's blocks | local |
| Remember the last open pane | Done | `Saved.prefs_pane` | local |
| **General:** theme (Auto, Light, Light Grey, Gray, Dark, Blue, Purple) | Done | `crates/falcon-app/src/theme.rs:73-83`, `prefs.rs:312-321` (live preview, reverted on Cancel) | local |
| General: language | Done | Preferences ▸ General ▸ Language (`Prefs::language`), Cancel reverts, `FALCON_LANG` override; whole interface converted | local |
| General: UI zoom level (50 to 150%) | Done | `Prefs::ui_zoom` in Eagle's steps, previewed live; egui's own keyboard zoom is disabled so ⌘+/⌘− only zoom the grid | local |
| General: transparency (vibrancy) effects | Partial | Preferences ▸ General checkbox, persisted as `Prefs::vibrancy`, applied at once (`vibrancy.rs`): NSVisualEffectView behind the `CAMetalLayer`, sidebar and inspector at 75% alpha, everything else opaque; OFF by default, because Eagle's `general.enableVibrancy` only blurs its menus and boxes while its main window stays opaque. Gaps: falcon's effect is on the side panels, not Eagle's menus and boxes, and it has not yet been seen on screen | local |
| General: show counts in the sidebar | Done | `Prefs::sidebar_counts` | local |
| General: menu bar icon | Done | `tray.rs` status item with its General switch | local |
| General: keep the Dock icon after the window closes | Done | Pref on by default (Eagle's `keepDockIcon`); off leaves the Dock on close while the menu bar icon is shown, or quits without one | local |
| General: launch at login | Done | `SMAppService` on macOS 13+, read from the system, **off by default**, never registered until the user turns it on; shown unavailable before 13 or unbundled | local |
| General: GPU acceleration | Missing | Not built: eframe's `glow` backend isn't compiled in and the 3D viewer needs wgpu; no switch to offer | local |
| General: auto-select items after import | Done | `Prefs::auto_select` | local |
| General: notify on duplicate import | Done | `Prefs::notify_duplicates` | local |
| General: ask where to save items from the extension | Missing | Not applicable: the browser extension is an online feature, and falcon has no extension | online/plugin |
| General: @2x screenshots | Done | `Prefs` `useRetina` equivalent, shown in General and Screenshot | local |
| General: IPTC keywords as tags | Done | `prefs.rs:326-328` | local |
| General (falcon only): show the sidebar and inspector | Done (beyond Eagle) | `prefs.rs:322-325` | local |
| **Sidebar:** double-clicking a sidebar item renames it or expands and collapses it | Done | `Prefs::sidebar_double_click` (default: expand/collapse) | local |
| Sidebar: show or hide Uncategorized, Untagged, Recently Used, Random, Community, Quick Access, Smart Folders and Folders | Done | `SidebarSections`, all on by default; no Community entry | local |
| **Controls:** mouse wheel in the viewer scrolls, pages to previous or next, or zooms | Done | `prefs.wheel` | local |
| Controls: hover zoom button on cards | Done | `prefs.rs:331-337` | local |
| Controls: double-clicking an item opens it in Eagle or in the default app | Done | `Prefs::double_click` | local |
| Controls: middle button does nothing, opens a new window or runs a plugin | Done | `Prefs::middle_button`: new window or nothing (plugins don't exist) | local |
| Controls: spacebar previews, opens Quick Look or scrolls the list | Done | `Prefs::space` | local |
| **Preview:** zoom rendering smooth or pixelated | Done | `prefs.pixelated` | local |
| Preview: remember the last zoom and position | Done | `prefs.remember_zoom` | local |
| Preview: default image size, smart zoom or original | Done | `prefs.open_actual_size` | local |
| Preview: rotation applies to the file or to the preview only | Done | `RotateMode` preference in the Image block; preview mode only changes the on-screen transform | local |
| Preview: transparency grid on or off | Done | `prefs.transparency_grid` | local |
| Preview: video vertical scroll controls progress or volume | Done | `Prefs::video_scroll` | local |
| Preview: video hover preview | Done | `Prefs::video_hover`, on by default | local |
| Preview: stretch video to fit the window | Done | `Prefs::video_fill` | local |
| Preview: video autoplay | Done | `Prefs::video_autoplay` | local |
| Preview: remember the playback position | Done | `Prefs::video_resume`, saved per item across launches | local |
| Preview: loop clips shorter than 30 s | Done | `Prefs::video_loop_short` | local |
| Preview: autoplay GIF, WebP and AVIF in the grid | Done | `prefs.always_play_animated`; AVIF on macOS | local |
| Preview: advanced GIF viewer | Done | `prefs.gif_player` | local |
| Preview: automatic font style tags | Done | `Prefs::font_style_tags` | local |
| **Screenshot:** format PNG, JPG or WebP | Done | Screenshot pane: PNG / JPG / WebP, default PNG | local |
| Screenshot: quality | Done | High 90 / Medium 80 / Low 70, shown for JPG and WebP | local |
| Screenshot: enable the global shortcut, plus area, window and full-screen shortcuts | Done | Enable switch (default off, where Eagle's is on, because Eagle owns ⌥⌘E) plus the area / window / full-screen rows, recordable in the pane; refused chords shown on the row | local |
| Screenshot: ask where to save | Done | Collect-window prompt for area and window shots | local |
| Screenshot: notify when taken | Partial | `capture_notice.rs`, `ui/capture_notice.rs`, `app/system.rs`, `notify.rs`. While falcon is the active app (`dock::is_active`) a 300 pt pop-up with the shot's thumbnail, title and name slides in at the window's top right, stays 1.5 s (longer while hovered), is replaced by the next shot, has a close button, and a click opens All with the item selected; otherwise the macOS notification (bundled only). Routing, expiry, replacement and thumbnail sizing are unit tested. Gap: not yet seen on screen; Eagle's drag-to-dismiss and its separate screen-level window (falcon's card is inside the window) are not built; the card shows the name where Eagle shows a hint line | local |
| Screenshot: auto-tag "Screenshot" | Done | "Screenshot" tag | local |
| Screenshot: also copy to the clipboard | Done | The finished picture is copied with arboard (not `-c`) | local |
| Screenshot: @2x on Retina | Done | Halves at scale ≥ 1.5 (Eagle checks exactly 2) | local |
| **Shortcuts:** rebind any command, search the list, Restore Defaults | Partial | 166 rebindable rows, 132 of them with Eagle's ids (of the 150 in its Shortcuts pane; 129 of the 147 in `default-preferences.js`; Eagle's `library.load` and `organize.folder.add` have rows), search field, Restore Defaults, recorder. `find.filter.import` (Eagle's `toggleFilterByType('import')` clicks its Date Modified filter, `mtime-filter-item`) opens falcon's Date chip on its Modified tab, and `find.filter.other` (`openFilterAddContextMenu`) opens the filter bar's + menu; both unbound as in Eagle, with Find ▸ Open Filter ▸ Date Modified and More Filters entries, and Find ▸ Date now starts on the Imported tab as Eagle's Date Added. `view.file.openother` opens View ▸ Open With Another App…, unbound; Eagle's Shortcuts pane lists it for `win32` only, while falcon also gives it a row on macOS. 18 Eagle ids have no row: the five Windows variants, the Pinterest and ArtStation importers, the eight reverse image searches and `find.filter.semantic` (online), `edit.image.merge` (Combine Images plugin), and `find.filter.image`: it runs `eagle.aiSearch.searchByItemId` / `searchByBase64` (`lib/api/ai-search.js`) against Eagle's AI Search plugin service at `http://localhost:38766`, a local model with a plugin to install, which falcon doesn't have (see the AI Search rows). The new menu items and the app chooser are not yet seen on screen | local |
| Shortcuts: plugin shortcuts | Missing | Not applicable: no plugin system | local |
| Global shortcuts (show Eagle, quick search from anywhere) | Done | `hotkeys.rs`, Global group in the shortcut table | local |
| **Notification:** sound effects master switch, plus per event (delete item, delete folder, screenshot, extension) | Partial | Preferences ▸ Notification: Play sound effects master switch plus delete item, delete folder and screenshot switches, Eagle's defaults (all on), persisted as `Prefs::sounds`, reported in `application/info` as `notification.soundEffect`. Gap: the extension switch is kept and reported but not shown, since falcon has no browser extension | local |
| Notification: pop-up master switch, plus per event (screenshot, extension, duplicates, auto-import) | Done | Master switch plus duplicates, import done, auto-import, screenshot and export switches; the extension event has no local counterpart | local |
| **Password:** enable the app password lock, change the password, Lock Now, Touch ID | Done | Set, change, remove, hint, Lock Now ⌥⌘L, Unlock with Touch ID (shown when available, fingerprint checked when turned on, as Eagle) | local |
| **Auto-Import:** enable, choose folder, open folder | Done | `prefs.rs` `auto_import` pane: enable, Choose Folder…, Open Folder, Make It for the watched subfolder | local |
| **AI Search** pane (install, enable or disable the local semantic-search plugin) | Missing | — | online/plugin |
| **AI Models** pane (shared API keys for OpenAI, Anthropic, Gemini and so on) | Missing | — | online/plugin |
| **Eagle MCP** pane (install or open the MCP server plugin) | Missing | — | online/plugin |
| **Developer:** show, copy and regenerate the API token | Done | Developer pane: empty by default like Eagle, Generate when empty, Copy / Regenerate / link when set; an empty token means Host-check-only auth (`authorized` in `falcon-server`) | local |
| Developer (falcon only): API on or off, and first port | Done (beyond Eagle) | `prefs.rs:339-358` | local |
| Developer: plugin developer tools | Missing | Not applicable: no plugin system | local |
| **Proxy** (server address and port, used for downloads and web search) | Missing | — | online |
| Color space (sRGB, Display P3, unmanaged) | Partial | App menu ▸ Color Space after Preferences with Eagle's three choices, order and default (Unmanaged), persisted in prefs and applied to the window's `CAMetalLayer.colorspace` (found among the view's sublayers since 700506ff; before that the menu did nothing) at start and on change (`colorspace.rs`; re-applied each frame because wgpu resets it on resize), disabled while locked or Preferences is open. Gap: egui's pixels are sRGB content, so the choice changes how the display interprets the window; there is no per-image colour management, and the visible effect is still to be checked on screen | local |

### 7. HTTP API

Falcon serves the API on 127.0.0.1 from port 41596 upward (never 41595), while a library is open in the app or through `falcon serve` (`lib.rs:69`, `prefs.rs:12-15`, `src/main.rs:85-92`). Routes are registered in `crates/falcon-server/src/lib.rs:355-411`.

#### 7.1 v1

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| `GET /` and `/api/application/info` | Done | version, build, platform, `showCollectModal` and a `preferences` subset (`lib.rs`) | local |
| `GET /api/library/info` | Done | `lib.rs:438` | local |
| `GET /api/library/history` | Done | The app's recent-libraries list through `Handle::set_history` | local |
| `POST /api/library/switch` | Done | `lib.rs:450-461`; the app follows it (`app/library.rs` `follow_api_switch`) | local |
| `GET /api/library/icon` | Done | `lib.rs:462`, `v2.rs:920` | local |
| `GET /api/folder/list` | Done | `lib.rs:465` | local |
| `GET /api/folder/listRecent` | Done | `lib.rs:466` | local |
| `POST /api/folder/create`, `rename`, `update` | Done | `lib.rs:469-501` | local |
| `POST /api/folder/unlock` | Done | Unlock for the server's session, nothing written; `folder/list` then reports `isUnLock` and children | local |
| `GET /api/tag/all`, `list`, `listRecent`, `groups` | Done | `lib.rs:502-522` | local |
| `POST /api/item/addFromPath`, `addFromPaths` | Done | `lib.rs:369-370` (imports finish before the reply) | local |
| `POST /api/item/addFromURL`, `addFromURLs` | Done | `lib.rs:668-680,753-779` | online for http URLs |
| `POST /api/item/addBookmark` | Missing | Not registered | online |
| `POST /api/item/batchSave` | Missing | Not registered | online |
| `POST /api/item/update` | Done | `lib.rs:373` | local |
| `GET /api/item/info`, `thumbnail`, `list` | Done | `lib.rs:374-377,782-800` | local |
| `POST /api/item/moveToTrash` | Done | `lib.rs:376` | local |
| `POST /api/item/setCustomThumbnail` | Done | File work without the library lock (`thumbnail::custom_thumbnailed`), then only the changed keys written | local |
| `POST /api/item/refreshPalette`, `refreshThumbnail` | Done | `thumbnail::redrawn` off the lock; v1 replies with no data as Eagle | local |
| `GET /api/preferences/collect/on`, `/off` | Done | Served; `application/info` reflects it | local |
| `/item?id=`, `/folder?id=` and `/smart-folder?id=` redirects to `eagle://` links | Done | 302 to the `eagle://` link, or 404 with Eagle's `{"error":…}`; auth still applies (Eagle skips it there) | local |
| `POST /api/script/inject` (runs arbitrary JavaScript) | Missing (on purpose) | Deliberately left out; a security hole | local |
| `GET /api/check` (obfuscated remote-script loader) | Missing (on purpose) | Not library functionality | online |
| Token auth (`?token=` matching the Developer token) | Done | Served if the Host is right or `token` matches (query, or body on POST); the Host check is kept | local |
| Listen on all interfaces | Different (on purpose) | 127.0.0.1 only (`lib.rs:9,69`) | local |

#### 7.2 v2

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| `app/info` | Done | `lib.rs:432` | local |
| `library/info`, `history`, `switch`, `icon` | Done (history is per server, as in v1) | `lib.rs:379-382,439-463` | local |
| `item/get`, `query`, `countAll`, `add`, `update` | Done | `lib.rs:383-387,540` | local |
| `item/add` with `bookmarkURL` | Missing | `v2.rs:301-302` | online |
| `item/getComments`, `addComment`, `updateComment`, `removeComment` | Done | `lib.rs:388-391,541-544` | local |
| `item/setCustomThumbnail`, `item/refreshThumbnail` | Done | v2 shapes: `itemId`/`filePath`, `true`/`false` replies | local |
| `folder/get`, `create`, `update` | Done | `lib.rs:392-394,545-546` | local |
| `smartFolder/get`, `create`, `update`, `remove`, `getItems`, `getRules` | Done | `lib.rs:395-400,534-539` | local |
| `tag/get`, `getRecentTags`, `getStarredTags`, `update`, `merge` | Done | `lib.rs:401-405,523-527` | local |
| `tagGroup/get`, `create`, `update`, `remove`, `addTags`, `removeTags` | Done | `lib.rs:406-411,528-533` | local |
| `aiSearch/*` (isInstalled, isReady, isStarting, isSyncing, getSyncStatus, checkServiceHealth, searchByText, searchByBase64, searchByItemId) | Missing | — | online/plugin |
| Interactive API playground page | Done | `/api/v2/playground` (`crates/falcon-server/src/playground.html`) | local |

### 8. Integrations

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| `eagle://` URL scheme registered with macOS | Done | `CFBundleURLTypes` in `scripts/bundle.sh`; `links.rs` handles the get-URL Apple event | local |
| `eagle://item/<id>` opens the item | Done | Opens the item in the viewer inside its first folder (or All); also as a launch argument | local |
| `eagle://folder/<id>`, `eagle://smart-folder/<id>`, `eagle://openFolder/<id>` | Done | `links.rs`: all three open the folder in the app (Eagle's `openFolder` does the same) | local |
| Open `.eaglepack` files by double-clicking them | Partial | `CFBundleDocumentTypes` plus the open-documents event send packs to the package import (`open_files.rs`); not yet checked in a built bundle | local |
| Install a plugin by opening a `.eagleplugin` file | Missing | Not applicable: no plugin system | local |
| Open With: list the apps that can open a file | Done | `crates/falcon-app/src/open_with.rs` | local |
| AppleScript dictionary or macOS Services | — (not counted) | — | — |
| Browser extension (Chrome, Safari and others) sending images, video, bookmarks and batch saves to the app on port 41595 | Missing | Falcon never uses 41595, so the stock extension cannot reach it; bookmarks and batch save are also missing | online |
| Plugin system: window, inspector, format (thumbnail and preview) and background-service plugins | Missing | Out of scope: Eagle's plugin platform | local |
| Plugin Center (browse, install, update, disable or uninstall; sort and search) | Missing | — | online/plugin |
| Plugin development (create from template, import a local project, pack, publish, reload, DevTools) | Missing | Out of scope: Eagle's plugin platform | local |
| Actions (recorded multi-step automations with shortcuts) | Partial | Falcon's own implementation of a feature that is a plugin in Eagle (`ai-action`): seven rule steps (add/remove/clear tags, add/remove/clear folders, rating) run in order on the multi-selection as one undo step, with per-action app-wide shortcuts and the 30-item confirm. Gap: the plugin's AI steps (auto-tag, describe) need AI Models (see PLUGINS.md) | local |
| AI Search: semantic and similar-image search on the device | Missing | — | online/plugin (model download) |
| AI Models: shared model configuration | Missing | — | online/plugin |
| Eagle MCP server plugin | Missing | — | online/plugin |
| Reverse image search on web engines | Missing | — | online |
| Licence activation, trial, unregister, device management, licence recovery | Missing (not needed) | Falcon is free software | online |
| Check for updates, update dialog, minor update, Rosetta warning | Missing | — | online |
| Help: Error Logs and Export Debug Report | Done | Help menu; `~/Library/Logs/Falcon/falcon.log` | local |
| Help: help centre, changelogs, roadmap, feedback, tutorial, tips | Missing | Cross-ref menus audit | online |
| Menu bar (tray) icon with capture actions, Show Eagle and Quit | Done | `tray.rs` | local |
| Scripted launch options (`--search`, `--color`, `--tags`, `--select`, item ids, files) | Done (beyond Eagle) | `crates/falcon-app/src/main.rs:20-41` | local |
| Command-line tool (`import`, `new`, `folder`, `info`, `backup`, `export`, `import-pack`, `serve`) | Done (beyond Eagle) | `src/main.rs:17-93` | local |

### 9. Localisation

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Eight UI languages: English, Simplified Chinese, Traditional Chinese, Japanese, Korean, Spanish, German and Russian (picked automatically from the system language, or set in Preferences) | `i18n/index.js` `supportLanguage`; `i18n/{en,zh_CN,zh_TW,ja_JP,ko_KR,es_ES,de_DE,ru_RU}.js`; collect window `locales/*.json`; PH `general.language` | Done | `crate::locale` with falcon's own catalogues for all eight (`crates/falcon-app/locales/`), picked from the system language or set in Preferences, applied live to the whole interface incl. the native menu bar; translations are machine-written and listed for review in `locales/REVIEW.md` | local |
| Chinese pinyin search support (jieba dictionaries) | Not wanted (user decision) | Not built by decision (2026-11-01): no jieba dictionaries; the model still round-trips `pinyin` | local |

---

### Counts

Rows marked "not counted" are features Eagle 4.0.0 does not have.

- **Done:** 189, including 5 where falcon goes beyond Eagle and 1 with a small difference.
- **Partial:** 11.
- **Missing:** 34, including 3 left out on purpose (`script/inject`, `/api/check`, licensing).
- **Different on purpose:** 1 (the API listens on localhost only).

### Ranked Missing and Partial local items

Ranked by how often a user would hit the gap, and the risk if they do.

1. ~~**Dropping a `.library` folder imports its insides as items.** It should open the library or be refused. This is a data hazard.~~ Fixed.
2. ~~**Folder import keeps no structure.** Dropped folders are flattened into the current folder, and there is no "Import Folders" dialog.~~ Fixed.
3. ~~No duplicate check on import.~~ Fixed.
4. ~~**Auto-backup and restore are not wired into the app.** Backups happen only through the CLI, and the app opens with `Library::open` rather than `open_or_restore`, so a damaged `metadata.json` makes the library unusable.~~ Fixed.
5. ~~**No Export to Computer for items or folders.** The Export button is disabled, and overwrite prompts are missing.~~ Fixed.
6. ~~**No external-change reload or sync-conflict detection.** Nothing watches `metadata.json` or `mtime.json`. This is risky with Dropbox, iCloud or a second app, including Eagle itself.~~ Fixed.
7. ~~**The import error panel is missing.** Falcon shows a count only, with no per-file reasons, retry or copy-out.~~ Fixed.
8. ~~Watched auto-import folder is missing.~~ Fixed.
9. ~~Folder auto-tags are not applied on import or paste.~~ Fixed.
10. ~~**`.eaglepack` import and export are missing from the app** (CLI only). Smart-folder packs are missing entirely, and dropping a pack imports the pack file itself as an item.~~ Fixed.
11. ~~**Merge another library and Add to Other Library are missing.**~~ Fixed.
12. ~~**Screen capture is missing**: area, window and full screen, global shortcuts, and the whole Screenshot pane.~~ Fixed.
13. ~~**The copy variants are missing**: name, folder path, thumbnail, base64, Eagle link and source URL. Copy File Path is context-menu only, for one item.~~ Fixed.
14. ~~**Export to CSV is missing.**~~ Fixed.
15. **The `eagle://` scheme and document types** are registered in the bundle. Partial: opening `.eaglepack` files by double-click and Finder's Open With are not yet checked in a built bundle.
16. ~~**Unsupported formats are imported silently** instead of being refused with a message.~~ Fixed.
17. ~~**The collect window is thin**: no multi-folder, recent folders, new folder, name, description or tags.~~ Fixed.
18. ~~**Library list management is missing**: clear history, remove from list, library icon, and the missing-library retry or relocate screen.~~ Fixed.
19. ~~App password lock has no Touch ID.~~ Fixed, and for locked folders too.
20. ~~Missing API routes.~~ Fixed.
21. ~~**Hard-coded behaviours need preference switches**: font style tag, video autoplay, loop short clips, hover video preview, saved playback position, sidebar counts.~~ Fixed.
22. ~~**Missing Controls and Preview settings**: middle button, and the image settings not yet wired (see §6).~~ Fixed.
23. ~~**Sidebar pane is missing**: show or hide sections, and double-click to rename or collapse.~~ Fixed.
24. ~~**Notifications**: system pop-ups and sound effects per event are done; the Notification Center is still open.~~ Fixed; the Notification Center is not an Eagle 4 feature, and falcon has one as an addition (main window §11).
25. **Settings**: vibrancy, the screenshot notice, the App menu's Color Space and the sound effects (extension switch hidden) are Partial; GPU acceleration is not built (no switch to offer); plugin shortcuts and plugin developer tools wait on the plugin system. Language, UI zoom, launch at login, Dock icon, menu bar icon and auto-select after import are built.
26. ~~**Localisation**: English only, against Eagle's eight languages.~~ Fixed.
27. ~~**Library safety checks are missing**: newer-version refusal, warnings for Desktop or Documents paths, read-only paths and slow disks, disk-space checks, and confirmation for large folders or deep trees.~~ Fixed.
28. ~~"Empty cache and reload" is missing.~~ Fixed, and "Clear Invalid Content" is built (`ui/dialogs/clear_invalid.rs`).
29. **Smaller gaps still open**: the plugin system and plugin development (out of scope) and the Color Space menu (Partial, not seen on screen). Loading progress, the welcome flow, Preferences search, the last pane, the Share sheet, WebP conversion, saving the video frame (Partial in the preview window), the menu bar icon, Inboard import, the API playground, the debug report and error logs are built.
