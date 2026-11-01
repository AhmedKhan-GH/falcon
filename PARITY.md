# Parity with Eagle

This is the checklist of everything Eagle 4.0.0 can do, compared with falcon. It was built on 2026-11-01 from Eagle's shipped app code and its English UI text, and every falcon status was checked in falcon's code, not taken from ROADMAP or the docs. Local parity means every row marked local is Done. Rows marked online or plugin need a network service, an account or Eagle's plugin system, and don't count.

Statuses: **Done** works as in Eagle. **Partial** exists but is missing options or behaves differently. **Missing** isn't there.

| Area | Done | Partial | Missing (local) | Missing (online/plugin) |
|---|---|---|---|---|
| Main window: menus, shortcuts, toolbar, sidebar, layouts | 217 | 37 | 18 | 8 |
| Viewers and file formats | 171 | 23 | 9 | 5 |
| Search, filters and organising | 207 | 8 | 7 | 3 |
| Import, export, library, preferences and integrations | 175 | 12 | 22 | 26 |
| **Total** | 770 | 80 | 56 | 42 |
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
2. ~~No application menu bar.~~ Fixed: native File / Edit / Find / Organize / View / Window / Help menus with live shortcuts; no Actions menu.
3. Preferences: 8 panes and about 30 settings against Eagle's 13 panes and about 80 settings; the rest are listed in §6.
4. ~~The full-size image is capped at 3072 px.~~ Fixed: the original is tiled at full resolution; HEIC/AVIF/JXL still come from a 3072 px QuickLook render (no decoders).
5. ~~Folder import flattens structure; no duplicate check on import; no import error list.~~ Fixed.
6. ~~No export of items or folders, and no CSV export.~~ Fixed.
7. Shortcuts still missing: T, F, ⌘J Go to Folder, ⇧+arrows, ⌘D, ⌘T and the tag clipboard, ⇧D, ⇧T, F5, ⌘O and more (see §3).
8. ~~No marquee selection, no multi-select of folders, no grid display options.~~ Fixed.
9. ~~No tag picker or suggestions.~~ Fixed.
10. ~~No GIF player controls, no animated WebP or AVIF, no GIF hover previews.~~ Fixed (AVIF on macOS); still no hover playback for video or audio.
11. ~~No interactive 3D viewer, text editor or full font viewer.~~ Fixed for obj/stl/ply/glb/gltf/3mf; fbx, 3ds, dae and ifc stay stills.
12. ~~No watched import folder, no reload on outside changes, no sync-conflict detection, no screen capture.~~ Fixed.
13. ~~English only.~~ Fixed in part: falcon's own translations into Eagle's eight languages with a live language preference; the viewers, sidebar and context menus still need converting.

The full tables follow. Each section ends with that area's own ranked gap list.

## Main window: menus, shortcuts, toolbar, sidebar, layouts


Scope: application menus, context menus, keyboard shortcuts, toolbar, sidebar, grid layouts, selection, drag and drop, window behaviours, themes, status UI, onboarding and empty states.

Paths: Eagle paths are relative to `eagle-src/app/` (`bundle` = `app/app.bundle.js`, `prefs` = `app/js/default-preferences.js`, `index` = `app/index.html`, `layout-panel` = `app/js/directives/layout-panel.html`, `i18n` = `i18n/en.js`). Falcon paths are relative to `crates/falcon-app/src/`.

Every falcon status here was checked in the code. ROADMAP and docs were not used. Rows that belong to other audits (viewers, search and filters, import, export, library, preferences, API) appear only as cross-references at the end of each section and are **not counted**.

Overall finding: falcon now has a native menu bar (`crates/falcon-app/src/menu.rs`, AppKit through `objc2-app-kit`) with File, Edit, Find, Organize, View, Window and Help menus whose items show their live shortcuts, enable and disable with the app's state and carry checkmarks. It holds only commands falcon has; Eagle's Actions menu and online items are absent.

---

### 1. Application menubar

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| A menubar with File / Edit / Find / Organize / View / Actions / Window / Help menus, each item showing its live shortcut | Partial | Native AppKit menus in `crates/falcon-app/src/menu.rs` (File, Edit, Find, Organize, View, Window, Help) with live shortcuts, enabled rules and checkmarks; clicks run through `App::run_command`. No Actions menu; only commands falcon has | local |
| App menu: About | Done | `ui/dialogs/about.rs`, replacing winit's stock item | local |
| App menu: Preferences… (⌘,) | Done | shortcuts.rs `app.preferences`; in the app menu after About (`menu.rs`) and in the library switcher | local |
| App menu: Lock Now (⌘⌥L, locks the whole app behind a password or Touch ID) | Done | ⌥⌘L locks the app (`applock.rs`); unlock by password or Touch ID (`touch_id.rs`, LAContext through `msg_send!`, pref `touch_id` shown only when available) | local |
| App menu: Color Space (sRGB / Display P3 / Unmanaged) | Missing | No colour-space choice | local |
| App menu: Hide, Hide Others, Quit | Done | Supplied by winit's default macOS app menu (main.rs uses eframe defaults) | local |
| App menu: Check for Updates, Register / Unregister, Manage devices | Missing | Not applicable to falcon | online/plugin |
| File: New… (⌘N), a searchable menu of folder, smart folder, imports, duplicates and new documents from templates (Sketch, PSD, AI, XD, TXT, Word, PPT, Excel, Keynote, Pages, Numbers, MindNode, XMind) | Partial | File ▸ New ▸ Text File only (`menu.rs`, `app/text_view.rs` `new_text_file`); no searchable menu or other templates | local |
| File: New Folder (⇧⌘N) | Done | A sibling right after the open folder, or top level when none is open, as Eagle's `newFolder` (`Action::CreateSibling`) | local |
| File: New Subfolder (⌥N) | Done | ⌥N, File menu and folder menu; inserted first among its siblings, as Eagle | local |
| File: New Smart Folder (⇧⌥⌘N) | Done | shortcuts.rs; opens the smart folder dialog | local |
| Edit: Undo / Redo | Done | ⌘Z and ⇧⌘Z with a capped transaction history; Edit menu entries go to a focused text field first | local |
| Edit: Cut / Copy / Paste / Select All | Partial | Menu entries exist and act on a focused text field; ⌘C copies files, ⌘V imports, ⌘A selects all. Cut works only in text fields | local |
| Edit: Rename (⌘R on macOS) | Done | ⌘R (F2 alias): one item renames through the inspector title; several open the Rename N Items dialog with Eagle's templates (`rename.rs`) | local |
| Edit: Copy File Path (⌥⌘C) | Done | Key and item menu copy every selected path, one per line (`export_ui.rs` `copy_variant`) | local |
| Edit: Copy Folder Path (⌥⇧⌘C), Copy Eagle Link, Copy Thumbnail, Copy Base64, Copy Name | Done | Edit ▸ Copy Details: Folder Paths, Links (`http://localhost:<port>/item?id=`, as Eagle's Copy Link), Eagle Links (`eagle://item/<id>`), Thumbnail, Base64, Names; all for every selected item | local |
| Edit: Duplicate item (⌘D) | Done | The first selected item, as Eagle's `duplicateItem`; the card entry is single-item only, as Eagle | local |
| Edit: Refresh Thumbnail (⌥⌘R), Custom thumbnail from a file (⌥⌘T), Reset custom thumbnail | Done | ⌥⌘R, ⌥⌘T, Edit ▸ Custom Thumbnail ▸ From File / From Clipboard / Reset (`edit.thumbnail.custom.reset`), card menu; Reset on the first item, as Eagle | local |
| Edit: Custom thumbnail from the clipboard (⇧⌥⌘T) | Done | Clipboard image or copied file | local |
| Edit ▸ Move Folder: Up, Down, To Top, To Bottom (⌘] ⌘[ ⇧⌘] ⇧⌘[) | Done | Keys and Edit menu entries move the open folder or smart folder among its siblings, undoably (`organize::arrange`) | local |
| Edit ▸ Edit Image: Horizontal Flip (⇧F), Rotate (⇧R), Crop (⇧C), Combine Images (⇧⌘M) | Partial | Flip, Rotate and Crop (⇧C, `edit.image.crop`) in the menu and keys; Combine Images is a plugin in Eagle 4, not built | local |
| Edit ▸ Edit Folder: Set Current Folder (⇧⌘R), Move Folder, Password create / edit / remove / lock (⇧⌘L) | Done | ⇧⌘R Folder Settings, Edit ▸ Move Folder…, Edit ▸ Folder Password ▸ Create / Change / Remove / Lock Now (⇧⌘L), with Eagle's ids and enabled states | local |
| Edit: Remove from current folder (⇧⌘⌫) | Done | shortcuts.rs `edit.remove.folder`, menu and card menu; one undo step, no prompt (Eagle's `removeFromFolder` asks nothing) | local |
| Edit: Move to Trash (⌘⌫) | Done | shortcuts.rs:114, app.rs:1095-1102 | local |
| Find: Go to Folder (⌘J or J), a quick-switch dialog with Folders / Smart Folders / Tags tabs | Done | `find.quicksearch` ⌘J and bare J: Folders / Tags / Smart Folders / Items tabs, ranked by `string_score`, recents (200 kept) | local |
| Find: Add to Folders… (⇧⌘J, a searchable folder picker that can create folders) | Done | `ui/dialogs/add_to_folders.rs`, `folder_picker.rs`: Find menu, ⇧⌘J, item menu and the inspector's + (one or several items); search, 8 recent folders, Create “…”, tree with keys; one undo step. Falcon's is a modal applied on Save where Eagle's popover applies on close | local |
| Organize: Add To Last Used Folder (⇧D) | Done | `organize.folder.addLast`: 50 recent folders kept app-wide, as Eagle's localStorage list | local |
| Organize: Rate 0–5 stars from the menu and keys | Done | Organize menu and the 0–5 keys (`organize.rating.*`) with a toast; in the viewer the bare digits keep their zoom meaning | local |
| Organize: Add Tags… (⌘T), Copy Tags (⇧⌘C), Paste Tags (⇧⌘V), Clear Tags | Done | Organize menu; shortcuts.rs `organize.tag.*` (Clear Tags unbound by default, as Eagle) | local |
| View: Always on Top (⇧T) | Done | `modes.rs` `toggle_on_top` through `ViewportCommand::WindowLevel`; View menu checkmark; off at launch as Eagle | local |
| View: jump to All / Uncategorized / Untagged / Recent / Random / All Tags / Trash (⌘1–⌘7) | Done | shortcuts.rs `view.all` … in Eagle's View menu order; from the viewer they close it first | local |
| View: layout Waterfall / Justified / Grid / List with checkmarks (⌥1–⌥4) | Done | ⌥1–⌥4 in Eagle's order; View menu entries with checkmarks (`menu.rs` `validateMenuItem`) | local |
| View: Scroll to Top / Bottom / Page Up / Page Down (Home, End, PgUp, PgDn) | Done | Home, End, Page Up and Page Down scroll the grid (a page is the view less 72 px, as Eagle); ⌘↑ / ⌘↓ select the first or last item | local |
| View: Zoom In / Out (⌘= / ⌘-) | Done | Thumbnail size (shortcuts.rs:96-97, app.rs:1024-1029) | local |
| View: Actual Size (⌘0) / Zoom to Fit (⌘9) | Partial | The viewer has 0 for fit and 1 for actual size, without ⌘ (app.rs:3510). No ⌘0 or ⌘9 | local |
| View: Grayscale preview (⌥⌘G) | Done | Every loader tier incl. grid cards, animations, video frames, PDF pages, font specimen and the preview window (`logic::grayscale_rgba`); custom thumbnails gray too, unlike Eagle | local |
| View: Open in New Window (⌘O) | Done | Every acted-on item except audio in a new window per open, stepped with ←/→ (`ui/viewer/preview_window/mod.rs` `open_preview_items`), as Eagle's one window that steps | local |
| View: Open With Default App (⇧↩), Open with Other App, Reveal in Finder (⌘↩) | Done | open_ui.rs:33-46, shortcuts.rs:104-105 | local |
| View: Open Link in Browser (⇧⌘O) | Done | `view.file.openlink`, `urls.rs` (web URL or file path) | local |
| View: show or hide Sidebar (⌥⌘1), Inspector (⌥⌘2), both (⌥⌘3) | Done | shortcuts.rs `view.toggle.sidebar`, `view.toggle.inspector`, `view.toggle.all`; Tab also toggles both; all work in the viewer too | local |
| View: Show Name (⌥⌘4), Show Item Info (⌥⌘5), Show Comment count (⌥⌘6) | Done | shortcuts.rs `view.toggle.listname/listmetas/listannotation`, View menu checkmarks, `logic::Display` | local |
| View: Show Subfolder Content (⌥⌘7) | Done | `view.toggle.subfolder`, `logic::folder_members`, `grid_order` in organize_ui.rs; the layout panel and the strip heading have the switch too | local |
| View: Enter / Leave Presentation Mode (F5) | Done | `view.toggle.slideshow`: full screen with panels and the viewer bar hidden; arrows step, Esc leaves. Eagle has no timer or shuffle either | local |
| Actions menu: user-defined Actions, each with its own shortcut | Missing | The toolbar ⚡ button is drawn but disabled (app.rs:2038) | local |
| Window: Toggle Full Screen, Minimize, Close Window (⌘W) | Done | ⌃⌘F, ⌘M and ⌘W through eframe viewport commands; ⌘W closes the viewer first, then the window (which quits falcon, like the red button) | local |
| Help: Help Center, Show Tutorial, Install Extensions, Feedback, Recover license, Change Logs | Missing | Not applicable | online/plugin |
| Help: Error Logs, Export Debug Report… | Done | Errors and panics logged to `~/Library/Logs/Falcon`; Help ▸ Error Logs and Export Debug Report (a zip of system, library and log details; no network or disk tests) | local |

Cross-references (not counted): File ▸ Import Folders / Links / Eagle Package / Pinterest / ArtStation / Auto Import, File ▸ Export to Computer / .eaglepack / Specified Format / CSV, Library menu (Create / Switch / Merge / Clear History / Empty cache and reload) and Find ▸ Search / Search All / Filter submenu / Search by Image / Find Duplicates belong to the import, export, library and search audits. Falcon's library switcher popup (app.rs:1595-1652) covers Open, Recent, New, Show in Finder, Collect Window and Preferences.

---

### 2. Context menus

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Every context menu has a type-to-filter search box with keyword matching | Partial | `ui/menus/search.rs` on the item, folder, smart folder and tag menus incl. submenus; Eagle also searches the palette, tag-group, New, filter +, saved-filter and Help menus | local |
| **Item**: Restore (in the Trash) | Done | app.rs:2531-2534, which also offers Delete Permanently | local |
| Item: Open Item(s) in New Window | Done | Card menu opens all its targets in one window stepping with ←/→; middle click opens its card | local |
| Item: Open With Default App / Open With ▸ apps / Reveal in Finder | Done | open_ui.rs:33-46 | local |
| Item: Open File Location ▸ All or one of the item's folders (jumps to the item there) | Done | Card menu; jumps to the item in that view | local |
| Item: Plugin ▸ recent plugins | Missing |  | online/plugin |
| Item: Add To Last Used Folder | Done | Card menu, ⇧D | local |
| Item: Add to Folders… (picker panel) | Done | Opens the Add to Folders picker | local |
| Item: Share (macOS share sheet) | Done | `share.rs` through `NSSharingServicePicker` at runtime | local |
| Item: Pin to Top / Unpin from Top inside a folder | Done | `item_ops.rs`: item key `pinned: {folderId: ms}`; pinned items sort first unless the folder is sorted at random; no pin badge on cards yet | local |
| Item: Set as Folder Cover | Done | image_edit.rs:90-95 | local |
| Item: New Folder with Selection (N items) | Done | `folders_ui` `new_with_dialog`; the new folder becomes the last-used folder for ⇧D | local |
| Item: Rename / Rename N Items (batch rename dialog) | Done | Rename N Items dialog (`rename.rs`, incl. `%F`), asks first at 10+ items; one item uses the inspector title | local |
| Item: Copy (files) | Done | Edit ▸ Copy, the item menu's Copy and ⌘C copy the selected files (`ui/dialogs/export.rs` `copy_items`) | local |
| Item: Copy File Path | Done | `export_ui.rs` `copy_variant`, every selected item | local |
| Item: Copy… ▸ Link, Folder path, Thumbnail, Base64, Name | Done | `export_ui.rs` `item_copy_menu`; Link copies the item's source URL | local |
| Item: Copy Tags / Paste Tags | Done | `item_menu` in app.rs | local |
| Item: Combine Images (several selected) | Missing | A plugin in Eagle 4 (`combine-images`); not built | local |
| Item: Duplicate | Partial | ⌘D, Edit menu and card menu duplicate one item (`item_ops.rs`); several selected are not all duplicated yet | local |
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
| Item: Move to Trash | Done | app.rs:2538 | local |
| Item (falcon only): Rotate Left / Right, Flip H / V, Crop… | Done | image_edit.rs:110-132 (falcon extra) | local |
| **Folder**: New Folder (sibling) | Done | `Action::CreateSibling`, right after the clicked folder under the same parent | local |
| Folder: New Subfolder | Done | app.rs:1836 | local |
| Folder: Move Folder… (picker dialog) | Done | `folders_ui` `move_dialog` with destination search; `Library::move_folders` (above, into or below a target) | local |
| Folder: Add to / Remove from Quick Access | Done | app.rs:1842-1848 | local |
| Folder: Rename | Done | Inline rename (app.rs:1839, 1797-1823) | local |
| Folder: Copy Link (eagle:// folder link) | Done | `eagle://folder/<id>` (Eagle copies its localhost URL); smart folders get `eagle://smart-folder/<id>` | local |
| Folder: Set Auto-Tag | Done | "Auto-Tags…" in the folder menu and ⇧⌘R (`organize_ui.rs` auto_tag_dialog) | local |
| Folder: Password ▸ Create / Edit / Remove / Lock Now | Done | organize_ui.rs:270-296 | local |
| Folder: Sort By ▸ Title A-Z / Z-A, current level or all levels | Done | organize_ui.rs:327-336 (falcon asks to confirm, like Eagle) | local |
| Folder: Expand/Collapse ▸ this folder, same level, all folders | Done | Expand/collapse entries in the folder menu; ⌘/⌥/⌘⌥ clicks on carets | local |
| Folder: Duplicate | Done | `Library::duplicate_folder`: copy placed after the original, new ids, no password, no items | local |
| Folder: Show Subfolder Content | Partial | In View, the layout panel, the strip switch and ⌥⌘7; not in the folder context menu, where Eagle has it | local |
| Folder: Change Icon… and colour row | Done | organize_ui.rs:308-320 | local |
| Folder: item sort for this folder (Order By) | Done | "Sort Items By" (organize_ui.rs:300) | local |
| Folder: Remove Folder | Done | Asks only when the folder has items or subfolders, with a ticked "trash the items too" box that can be unticked; disabled while locked | local |
| **Several folders selected**: menu with batch rename, shared order, move, quick access, export, icon, colour, remove | Done | `multi_folder_menu` in `folders_ui`: batch rename (Eagle's templates, `rename.rs`), shared sort, Move Folder…, Quick Access, Export to Computer, icon and colour for all, remove. Falcon's icon grid instead of emoji | local |
| **Smart folder**: New Smart Folder (sibling) / New Subfolder / Rename / Edit Rules / Quick Access / Change Icon / Remove | Done | organize_ui.rs:372-423 | local |
| Smart folder: Duplicate | Done | `smart::duplicate_smart_folder`: deep copy placed before the original, `parent` keys fixed | local |
| Smart folder: Refresh | Missing | Not in Eagle 4's smart-folder menu either; not applicable | local |
| **Smart Folders heading +**: New Smart Folder / New Group | Done | organize_ui.rs:488-505 | local |
| **Quick Access item**: Remove from Quick Access | Done | app.rs:1692-1696 | local |
| **Subfolder card** (strip above a folder's items): Rename / Auto-Tag / Password / Export / Remove | Done | Card menu: Rename, Auto-Tags…, Password ▸, Export ▸, Delete Folder (⌘⌫); Rename/Auto-Tags/Delete disabled and Export hidden while locked, as Eagle | local |
| **Trash** sidebar item: Empty Trash | Done | app.rs:1758-1763 | local |
| Trash sidebar item: Restore All Items | Done | Trash menu, after asking | local |
| **Recently Used** item: Clear history | Done | Recently Used menu | local |
| **Sidebar nav area**: choose which entries are visible (All, Uncategorized, Untagged, Recent, Random, Community, All Tags) | Missing |  | local |
| **Sidebar / panel edges**: Toggle Sidebar / Inspector / both | Done | View menu, ⌥⌘1/2/3, the toolbar panels button and its right-click menu | local |
| **Folders heading**: Sort folders by title | Done | app.rs:1713-1715 | local |
| **Empty grid** right-click opens the layout and sort panel | Missing | The background only clears the selection (app.rs:2490-2493) | local |
| **List-view column header** right-click picks columns (tags, dimensions, rating, extension, file size, date added) | Done | `layout_panel::list_header` | local |
| **Toolbar title** right-click lists the ancestor folders to jump to | Missing |  | local |
| **Tag** (All Tags page): Rename / Star / Move to Group / Remove from Group / Remove | Done | app.rs:2849-2861, organize_ui.rs:919-940 | local |
| Tag: Filter items with the tag(s), Merge N tags (several selected), Add to several groups | Done | `tags_menu` in app.rs, `merge_dialog` (2–5 tags as Eagle), `Library::merge_tags`, `add_tags_to_group_too` | local |
| **Tag group**: Rename / Remove | Done | organize_ui.rs:820-845, which adds description, move and colour | local |
| Tag group: Find items with any tag in the group | Done | Group heading menu, `filter_any_tags` | local |
| **Tag list sort**: A>Z / Z>A / by count / group by Alphabet or Groups | Partial | Sort menu A>Z / Z>A / by count and group-by toggle; the choice isn't saved across launches, Eagle keeps it | local |
| **Palette swatch** (inspector): Find similar colour, copy as HEX / RGB / RGBA / HSL / HSV / HWB / CMYK, set as primary | Done | Find Similar, copy as HEX/RGB/RGBA/HSL/HSV/HWB/CMYK (`colors.rs`), Primary Colour toggle with a ring on the primary swatch | local |
| **Inspector tag input**: Copy / Paste / Clear All Tags | Done | `tag_area_menu` in `app/tag_picker.rs` | local |

---

### 3. Keyboard shortcuts

Eagle defaults come from prefs `shortcuts.keybinds` (rebindable) and from the hard-coded Mousetrap table at bundle 49291-49340.

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Shortcut editor: rebind, conflict check, clear, reset, grouped list | Done | shortcuts.rs:150-200, 400-530 | local |
| Shortcut editor: search field ("Search Shortcut") | Missing | No filter in the shortcut list | local |
| Shortcut editor coverage: the roughly 150 rebindable commands | Partial | 60 commands plus 12 player keys (shortcuts.rs TABLE) | local |
| ⌘F search, ⇧⌘F toggle filter | Done | shortcuts.rs:108-109 | local |
| ⌥⌘F search all, ⌥⇧F filter sidebar, ⇧⌥⌘F reset filter, ⌥F / ⌥T / ⌥C / ⌥S / ⌥R / ⌥D / ⌥E open one filter | Done | `find.search.all`, `find.sidebar.filter`, `find.filter.reset`, `find.filter.*` | local |
| ⌘L switch library | Done | shortcuts.rs:117, app.rs:998 | local |
| ⌘E export, ⇧⌘E export .eaglepack, ⇧E export as format | Done | shortcuts.rs Export group | local |
| ⌘1–⌘7 sidebar views | Done | shortcuts.rs `view.all`, `view.unfiled`, … | local |
| ⌥1–⌥4 layouts | Done | shortcuts.rs `view.layout.*`, in Eagle's order | local |
| ⌥⌘1–⌥⌘8 panel and caption toggles plus Navigator | Done | ⌥⌘1–⌥⌘7 and ⌥⌘8 Navigator (`view.toggle.navigator`) | local |
| ⇧T always on top, F5 slideshow, ⌥⌘G grayscale | Done | shortcuts.rs `view.alwaysOnTop`, `view.toggle.slideshow`, `view.grayscale` | local |
| ⌘O open in new window, ⇧⌘O open link | Done | ⌘O opens the acted-on items in a new window; ⇧⌘O opens the link | local |
| ⇧↩ default app, ⌘↩ reveal | Done | shortcuts.rs:104-105 | local |
| ⌥⌘S reveal, ⌥⌘D duplicate (view) | Missing |  | local |
| Home / End / PageUp / PageDown scroll the grid; ⌘↑ / ⌘↓ jump to top or bottom | Done | `run_command` scroll offsets; ⌘↑ / ⌘↓ also select the first or last item, where Eagle only scrolls | local |
| 0–5 set the rating of the selection | Partial | Grid and the text, font, video and audio viewers; in the image viewer 0 and 1 are falcon's zoom keys where Eagle rates | local |
| T opens the tag panel, F opens the folder panel, J opens Go to Folder, G opens Actions, P opens Plugins | Partial | T and J done; no F, G or P | local |
| R reshuffles the Random view | Done | R and View ▸ Shuffle Again; also a folder or smart folder sorted Random | local |
| `*` or `/` expand or collapse every folder | Done | Eagle's `toggleAllFolders` rule (smart folders when one is open); `*` is the ⇧8 alias chord | local |
| Tab toggles all panels | Done | Fixed key in shortcuts.rs FIXED, when no text field has focus (`logic::toggle_all`) | local |
| Space opens the quick preview (preference: preview, native Quick Look, or page down) | Done | `Prefs::space`: previews the first selected item (any selection), Quick Look, or page down (no selection needed) | local |
| ⇧Space pages up | Done | Fixed key | local |
| Enter toggles the viewer | Done | Enter is an alias of Viewer (shortcuts.rs:95) | local |
| Esc closes panel, slideshow or viewer, otherwise clears the selection | Done | app.rs:1076-1081 | local |
| Arrow keys move the selection; ←→ step in the viewer | Done | app.rs:1105-1135 | local |
| WASD as arrow keys | Done | Fixed keys | local |
| ⇧ + arrows extend the selection | Done | `logic::extend_selection`: ⇧←/→ grow or shrink from the anchor; ⇧↑/↓ only in List layout, as Eagle | local |
| Arrow keys move through the sidebar tree when it has focus (focus zones: content / sidebar / tags) | Partial | Content and sidebar zones; no tags zone (needs the All Tags tag-group sidebar) | local |
| ⌫ goes back (or leaves the viewer); ⌥← / ⌥→ and ⌘← / ⌘→ move through history | Done | shortcuts.rs `go.back` (⌫, ⌘←, ⌥←) and `go.forward` (⌘→, ⌥→); in the viewer ⌫ leaves it (app.rs `keyboard`) | local |
| ⌥↑ opens the parent folder | Done | Of the open folder or smart folder | local |
| ⌥⇧N creates a text file from a template; ⌥⇧C sets the folder cover | Partial | ⌥⇧N creates and opens an empty Untitled.txt (`file.create.txt`); ⌥⇧C is still context-menu only | local |
| ⌘W closes the window | Done | Closes the viewer first, then the window | local |
| ⌘Z undo | Done | app.rs:992 | local |
| ⌘A select all, ⌘C copy | Done | shortcuts.rs:110-112 | local |
| Delete (Windows) / ⌘⌫ trash; ⇧⌘⌫ remove from folder | Done | ⌘⌫ and the forward-delete key trash; ⇧⌘⌫ removes from the folder | local |
| ⌘T add tags, ⇧⌘C / ⇧⌘V copy and paste tags, ⇧D add to last folder, ⌘D duplicate | Done | `organize.tag.*`, `organize.folder.addLast` ⇧D, `edit.duplicate` ⌘D | local |
| ⇧⌘L lock the folder now, ⌥⌘L lock the app | Done | `edit.folder.lock` ⇧⌘L and `app.lock` ⌥⌘L | local |
| Hold Z for the hover preview | Done | app.rs:2703, hover.rs | local |
| Global shortcuts: Show Eagle, Bring up Eagle and search, screen captures (⌥⌘E) | Done | `hotkeys.rs` (Carbon `RegisterEventHotKey`): Show falcon, Show falcon and search, captures with ⌥⌘E default and the per-mode keys; released while Preferences is open; ANSI key positions | local |
| Falcon extras: ⇧⌘K collect window, ⌃Tab / ⌃⇧Tab next or previous open library | Done | shortcuts.rs `file.collect.window`, `library.next`, `library.previous` (falcon only) | local |

Viewer and player keys (frames, volume, speed, ` zoom toggle, N comment, M mute, C comment mode) belong to the viewer audit. Falcon has the player table at shortcuts.rs:122-134.

---

### 4. Toolbar

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Show-sidebar button when the sidebar is hidden | Done | app.rs:2006-2012 | local |
| Back / Forward buttons | Done | app.rs:2013-2018 | local |
| Title for the current view, "Search results", or "N folders selected" | Partial | Title, "Search results" and "N folders selected"; no breadcrumb, no result count, active filters ignored | local |
| Right-click the title for an ancestor jump menu | Missing |  | local |
| Thumbnail size slider with − and + | Done | app.rs:2085-2109 | local |
| Refresh button in the Random view | Partial | R and View ▸ Shuffle Again reshuffle; no on-screen Refresh button | local |
| Layout and sort panel button | Done | `app/layout_panel.rs` in Eagle's order (layout, Fit/Fill, sort, Show Name/Info/Extension/Label/Comments/Subfolders, sidebar and inspector, Refresh); no Speed/Quality | local |
| Filter button with an active state | Done | app.rs:2034, 2043-2049 | local |
| Search field with a scope menu (Tags / URL / Comment / Note) and a search-by-image camera | Partial | Search scope menu on the caret (`search_scope_menu`); the camera is disabled (search by image is online) | local |
| Actions button | Missing | Drawn disabled (app.rs:2038) | local |
| Plugins button | Missing | Drawn disabled (app.rs:2040) | online/plugin |
| AI Search button | Missing |  | online/plugin |
| Show-inspector button when the inspector is hidden | Done | app.rs:3642-3646 | local |
| "Move duplicates to trash" button on the duplicates page | Done | duplicates_view.rs (cross-ref) | local |
| Sidebar header "Add" button that opens the New… menu | Partial | The + in the sidebar header only creates a folder (app.rs:1591) | local |

---

### 5. Sidebar

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Library name and switcher at the top | Done | app.rs:1595-1652, which adds several open libraries (falcon extra) | local |
| Nav entries: All, Uncategorized, Untagged, Recently Used, Random, All Tags, Trash, each with a count | Done | app.rs:1740-1776, which adds Duplicates | local |
| Recently Used lists items the user recently opened | Done | Opened items newest first (viewer after a second, external open, Open With, new window, copy), kept per library in app storage as Eagle | local |
| Community entry | Missing |  | online/plugin |
| Show or hide each nav entry (context menu and preferences) | Done | Context menu on All / Untagged / Random and Preferences ▸ Sidebar | local |
| Show or hide the count badges | Done | `Prefs::sidebar_counts` | local |
| Quick Access section | Done | app.rs:1670-1699 | local |
| Section headers (Quick Access / Smart Folders / Folders) collapse on click and show a count | Done | Every header folds with a caret and shows its count (`SectionFold`, saved globally as Eagle) | local |
| Smart Folders tree with groups, nesting, icons, colours, counts | Done | organize_ui.rs:340-450 | local |
| Folder tree with carets, guide lines, icon and colour tints, counts, lock badge | Done | app.rs:1778-1890 | local |
| Double-click a folder to expand or collapse (preference: or rename) | Done | `Prefs::sidebar_double_click`, folders and smart folders | local |
| Select several folders with ⌘ or ⇧ click to see their combined items | Done | `logic::TreeSel` (⌘ toggles, ⇧ ranges among visible rows), `folders_ui`; the grid shows the union under the global sort | local |
| Filter field at the bottom of the sidebar | Done | app.rs:1656, 1719-1736. No ⌥⇧F key | local |
| "Go to Folder" button | Done | Sidebar header ⇄ button opening the quick switcher, in Eagle's order (Create Folder, Go to Folder, Hide) | local |
| Resize the sidebar by dragging its edge | Done | 200–480 pt, kept across launches | local |
| Background task status: "Processing" or "Paused" palette analysis with Pause / Resume | Missing |  | local |
| Drop zone hint: "Drag and drop folder here to import" | Done | Falcon's own hint text over the grid, the sidebar and folder rows while files are dragged in | local |

---

### 6. Grid layouts and display options

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Four layouts: Waterfall, Justified, Grid, List | Done | logic.rs:224-262 | local |
| Default layout is Justified | Done | `logic::Layout` default, `logic::layout_for` | local |
| Layout remembered per folder | Done | `logic::layout_key`: per folder or smart folder, else per library (`Saved.layouts`), as Eagle's `eagle.list.layout.<id>` | local |
| Grid layout Fit or Fill option | Done | `Display.fill`, `draw_image_cover`; hover playback still works in Fill | local |
| Thumbnail Speed vs Quality (load originals for large tiles) | Done | Layout panel option, Quality by default as Eagle; large jpg/png/webp/bmp cards show the original up to 1600 px through a bounded `Tier::Card` cache (16 entries) | local |
| Sort-by choices in the layout panel, global and per folder, including Manual, Duration and Random | Done | Global sort with Duration and Random (`OrderBy::ALL`); a folder's own sort adds Manual in the panel (`own_sort_menu`) | local |
| Toggle Show Name under thumbnails | Partial | `Display.name`; no caption in Justified, which Eagle captions | local |
| Toggle item info, choosing Dimensions / File Size / Tags / Rating / Date Modified / Date Created | Partial | `logic::caption_info` with Eagle's six types; no caption in Justified, and no font-weight or URL-domain fallbacks | local |
| Toggle the file extension in the name | Partial | `logic::caption_name`; no caption in Justified | local |
| Toggle the extension label badge | Done | `Display.ext_badge` | local |
| Toggle the comment-count badge | Done | `Display.comments` on cards and in List rows (26 px gutter) | local |
| Show subfolder contents in a folder | Done | View menu, ⌥⌘7, the layout panel, the strip switch and the folder context menu | local |
| Show sidebar and Show inspector switches in the layout panel | Done | `layout_panel.rs` | local |
| Refresh button in the layout panel | Done | Flushes, re-reads the library from disk, forgets textures and reshuffles | local |
| List layout with configurable columns and click-to-sort headers | Done | `layout_panel::list_header`, `logic::ListColumns`, `header_sort`: Eagle's seven columns, click sorts (writes the folder's sort inside a folder), right-click picks columns | local |
| Subfolder cards above a folder's items, with covers | Done | covers.rs:65-140 | local |
| Collapse the subfolder strip | Partial | Click the strip heading; the strip is absent in the List layout | local |
| Rename a subfolder card by double-clicking its name | Partial | Double-click the card name; the strip is absent in the List layout | local |
| Thumbnail background (checkerboard for transparency, etc.) | Done | Card menu Thumbnail Background ▸ None/Black/Gray/White/Checkerboard, item key `background` with Eagle's values (`logic::ThumbBg`, `Action::Background`); drawn in grid, list and viewer | local |
| Hover zoom button and hold-Z preview | Done | app.rs:2680-2743, hover.rs | local |
| Video, GIF and audio play on hover in the grid | Partial | GIF and animated WebP cards play in place while hovered (`anim.rs` `Cards`, ⌥ keeps them still); video and audio don't | local |
| Pinned items stay at the top of a folder | Done | `item_ops::pinned_first` for the open folder unless sorted at random; Pin to Top / Unpin in the card menu; no pin badge on cards | local |
| Manual drag order in a folder | Done | organize_ui.rs:616-675 | local |
| Missing-file or long-path hint on broken thumbnails | Partial | Extension placeholder only (app.rs:2867) | local |

---

### 7. Selection

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Click selects one, ⌘-click toggles, ⇧-click selects a range | Done | app.rs:829-850 | local |
| Drag a rectangle over empty space to select (rubber band) | Done | `logic::rect_hits`, `marquee_selection`, `edge_scroll` in all four layouts; ⌘ XORs; ⇧ adds (Eagle nearly replaces) | local |
| ⌘A / Esc / click on empty space | Done | app.rs:1082, 1076, 2575 | local |
| ⇧ + arrows extend the selection | Done | `logic::extend_selection`: ⇧←/→ grow or shrink from the anchor; ⇧↑/↓ only in List layout, as Eagle | local |
| Right-click an unselected item selects it first | Done | app.rs:2504-2506 | local |
| After moving items out of a folder or to the Trash, the next item is selected | Done | `App::select_after_removal` for trash, restore, delete, Remove from Folder and drops | local |
| Auto-select items after import | Done | `Prefs::auto_select` (on by default): up to 1,000 new items shown in the view | local |
| Selected item is scrolled into view during keyboard moves | Done | app.rs:1134, 2468-2479 | local |
| Several items selected: the inspector shows a summary with batch tag, rating and trash | Done | app.rs:2938-3018 | local |

---

### 8. Drag and drop inside the window

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Drag items onto a sidebar folder | Partial | `logic::folder_drop` move/add rules with ⌥ and the toast; no password prompt for a locked target, and removal only from the open folder rather than every selected folder | local |
| Toast after a drop ("N items moved to Folder", with a link to the folder) | Done | `flash_folder` toast with the folder name as a link (`toast.rs` `Toast.link`) | local |
| Drag items onto a Quick Access folder | Partial | Drops accepted; same locked-folder and multi-folder gaps as sidebar rows | local |
| Drag items onto a subfolder card | Done | Same `DropOnFolder` path as sidebar rows, with the bulk gate | local |
| Drag items onto the Trash | Done | app.rs:1764-1767 | local |
| Drag folders to reorder or nest, with insertion lines | Done | organize_ui.rs:254-266 | local |
| Drag a folder onto the Folders heading to move it to the top level | Done | app.rs:1705-1711 | local |
| Drag smart folders to reorder or nest | Done | organize_ui.rs:362-372 | local |
| Drag Quick Access entries to reorder them | Done | Reorder by dragging; folders and smart folders can be dragged in | local |
| Drag items within a manually sorted folder to reorder them | Done | organize_ui.rs:616 | local |
| Drag items out to Finder or other apps | Done | app.rs:3699-3730 | local |
| Drag files from Finder onto the grid to import into the open folder | Done | app.rs:3601-3606, 852-862 | local |
| Drag files from Finder onto a specific sidebar folder | Done | `import_ui.rs` `drop_files` hit-tests the drop against folder rows (`drop_pos.rs`); files go into that folder, folders are ignored, as Eagle's `onDropFolder` | local |
| Drag a Finder folder onto the sidebar to import it with its folder tree | Done | `folder_import.rs` plan and `import_ui.rs` `import_folder_trees`: a drop on the sidebar's empty space makes a top-level folder tree and opens it; on the grid it imports flat, as Eagle's `onDropContainer` does | local |
| Drop an .eaglepack onto the window | Done | Grid, sidebar folder rows (into that folder) and empty sidebar space (into the open folder), as Eagle's `onDropSidebar` | local |
| Drop an image or URL from a browser | Missing |  | online/plugin |
| Drag tags onto tag groups or the starred list; ⌥ copies instead of moving | Missing | Cross-ref organising | local |
| Drag ghost showing the item count | Done | app.rs:3732-3740 | local |

---

### 9. Window behaviours

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Frameless window with traffic lights; drag the title area; double-click to zoom | Done | main.rs:45-51, app.rs:1510-1519 | local |
| Remember window size and position | Done | eframe persistence (Cargo.toml `persistence` feature) | local |
| Full screen | Done | ⌃⌘F and Window ▸ Full Screen, plus the green button | local |
| Always on top for the main window | Done | `modes.rs` | local |
| Open items in a separate preview window: own titlebar always-on-top toggle, opacity 10–100%, navigator, zoom presets 5–800%, grayscale, rotate / flip, copy, copy path / link, Reveal in Eagle | Missing |  | local |
| Middle-click opens an item in a new window (preference) | Done | `Prefs::middle_button` (new window, default, or nothing); never for audio, as Eagle | local |
| Presentation / slideshow mode | Done | `modes.rs`; `ViewportCommand::Fullscreen`, panels and bar hidden | local |
| Closing the window keeps the app running (Dock icon stays); reopen from the Dock | Done | Closing hides falcon (`NSApp hide:`, so the collect and preview windows hide too); the Dock icon or reopening brings it back; ⌘Q quits | local |
| Menu bar status icon: Show Eagle, Capture Area / Window / Full Screen, Quit | Done | `tray.rs` `NSStatusItem`: Show falcon, Capture Area / Window / Full Screen, Quit; General switch; rebuilt on language change | local |
| Lock screen for the whole app, with Touch ID | Done | Password and a Use Touch ID button (`touch_id.rs`) | local |
| Loading screen while a library opens (progress messages) | Partial | `Library::open_with_progress` for opens started in the app; Try Again / Select Another Path, the open after creating a library and launch restore still block | local |
| Resizable inspector | Done | Drag its inner edge; kept across launches | local |
| Several libraries open in one window (falcon only) | Done | app.rs:624-676, workspaces.rs (extra) | local |
| Floating collect window (falcon only) | Done | collect.rs (extra) | local |

---

### 10. Themes and appearance

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Themes: Auto, Light, Light Grey, Gray, Dark, Blue, Purple | Done | theme.rs:73-130, prefs.rs:312 | local |
| Auto theme follows the system | Done | theme.rs:365 `follow_system` | local |
| Transparency (vibrancy) effects | Missing |  | local |
| UI zoom level | Done | `Prefs::ui_zoom`, applied live | local |
| Interface language | Partial | `crate::locale`: Automatic or one of Eagle's eight languages (en, de, es, ja, ko, ru, zh_CN, zh_TW), applied live incl. the native menu bar; falcon's own translations (`crates/falcon-app/locales/`, unreviewed). The viewers, sidebar, context menus and the Auto-Import/Developer blocks are still English | local |
| File-type colours on extension badges | Done | theme.rs:447-468 | local |

---

### 11. Status and notification UI

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Toast messages (rename, remove, rate, copy path, pin and so on) | Done | `toast.rs`: brief, normal, undoable and error kinds with Eagle's durations; errors also go to the log | local |
| UNDO button on toasts | Done | `toast.rs` undoable kind | local |
| Clickable links in toasts (open the folder) | Done | `Toast.link` on the folder add and drop toasts | local |
| Import progress bar with count and time remaining | Partial | Count and file name (app.rs:911-940). No time remaining and no cancel | local |
| Import errors toast ("N files failed, click for details") opens an error list with retry and reveal | Partial | Toast shows the failure count only (app.rs:925) | local |
| Dock icon progress bar during long tasks | Partial | `dock_progress.rs` driven by the progress modal (thumbnails, Trash, package import); not by the background file import | local |
| Progress modals for export, eaglepack, thumbnail refresh, empty trash, WebP conversion and similar | Partial | Shared progress modal with Cancel for pack import and export, thumbnail refresh and Empty Trash; Export to Computer keeps its toolbar bar; no WebP conversion | local |
| Sound effects (delete item, delete folder, capture) | Missing |  | local |
| System notifications (import done, capture done) | Partial | `notify.rs` posts UserNotifications for import done, export done and capture done when falcon runs from a bundle (`dev.falcon.app`); dropped under `cargo run`; clicking one only activates falcon | local |
| Warnings: slow disk, NTFS on macOS, library path permissions, localhost / API port | Partial | API port errors appear in Preferences (prefs.rs:100). No disk or permission warnings | local |
| Tooltips on toolbar buttons | Done | app.rs:1492-1507 | local |

---

### 12. Onboarding and empty states

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Welcome flow: welcome, choose a theme, install browser extension, create or open a library with example names | Missing |  | local (extension step online) |
| No-library screen offers both Create and Open | Partial | Only "Choose Library…" (app.rs:3531-3546). Create is reachable from the switcher only after a library is open | local |
| Tutorial / tips overlay | Missing |  | local |
| Empty All or folder view: "Drag and Drop" with a hint | Done | Tailored empty states per view | local |
| Empty search: "Nothing Found, maybe another keyword" | Done | Tailored empty state | local |
| Empty Trash text | Done | Tailored empty state | local |
| Uncategorized and Untagged intro and "done" states | Done | Tailored intro and done states | local |
| No tags yet / no uncategorized tags | Done | "No Tags Yet", plus the all-grouped notice in Tag Group mode (`empty_tags_filed`); falcon has no separate Uncategorized tags view | local |
| Duplicates: none found | Done | duplicates_view.rs:199-207 | local |
| Locked-folder unlock screen | Partial | Password unlock screen (organize_ui.rs:676). No Touch ID | local |

---

### Missing and Partial local items, ranked by how noticeable they are

1. ~~No application menubar.~~ Fixed; no Actions menu.
2. Shortcuts missing: T (tags), F (folders), ⌘J Go to Folder, ⇧-arrows, ⌘T and copy/paste tags, ⇧D last folder, ⌘D duplicate, ⇧T always on top, F5, ⌘O new window.
3. ~~Layout, history and rename keys differ from Eagle.~~ Fixed; what remains is Home/End/PgUp/PgDn, ⌘1–7 and the other missing keys in §3.
4. ~~Dragging items between folders adds them instead of moving them.~~ Fixed; the drop toast still has no link to the folder.
5. ~~No rubber-band (marquee) selection.~~ Fixed.
6. Item context menu gaps: Rename / batch rename, Copy ▸ (link, folder path, thumbnail, base64, name), Copy / Paste Tags, Duplicate, Remove from this Folder, Open File Location, Pin to Top, New Folder with Selection, Add to Last Used Folder, Share, thumbnail background, custom thumbnail from clipboard, Replace with File, Combine Images.
7. Grid display options: only Thumbnail Speed / Quality is missing.
8. Add to Folders as a searchable picker instead of a flat list (Go to Folder is done).
9. No multi-folder selection in the sidebar.
10. Open in New Window (preview window with always-on-top, opacity and navigator), plus main-window Always on Top.
11. Presentation / slideshow mode (F5) and grayscale preview.
12. Folder context menu gaps: New Folder (sibling), Move Folder dialog, Expand/Collapse submenu, Duplicate, Copy Link, Show Subfolder Content. Also no menu at all on subfolder cards, and no menu for several folders.
13. ~~Fixed-width sidebar and inspector; nav entries fixed.~~ Fixed; Smart Folders and Folders section headers still don't fold.
14. ~~List layout: no column header, no column chooser, no sort-by-column.~~ Fixed.
15. ~~Empty states are a single "No items".~~ Fixed; the welcome flow and the no-library screen are the library builder's.
16. Toast system: ~~no UNDO button, no links, no import error list~~ fixed; no dock progress, no time remaining.
17. Drag and drop gaps: files onto a specific sidebar folder, a Finder folder onto the sidebar (keeping its tree), items onto Quick Access or subfolder cards, Quick Access reorder.
18. ~~Context menus have no type-to-filter search.~~ Fixed (not inside submenus).
19. ~~Trash ▸ Restore All, Recently Used ▸ Clear history, Recently Used by modification time.~~ Fixed.
20. Right-click on the empty grid (layout panel), right-click on the title (ancestor jump), ⌥↑ to the parent folder, R to reshuffle Random and a refresh button.
21. Video, GIF and audio hover-play in the grid.
22. Window: closing now hides falcon (whole app) and the Dock icon brings it back; Full Screen and the app lock exist.
23. Smaller items: Color Space menu, vibrancy, sound effects, palette-analysis status indicator, shortcut-list search is done, Error Logs / Debug Report are done.

## Viewers and file formats


Scope: every viewer, its controls, where it opens, how each format gets a thumbnail and a preview, and grid hover behaviour. Eagle evidence is from the extracted app (`app/index.html`, `app/app.bundle.js`, the viewer folders, `app/js/*`, `config.js`, `i18n/en.js`). Falcon evidence is from the code only (no docs or ROADMAP trusted).

Paths: `E/` = `eagle-src/app/app/`, `i18n` = `eagle-src/app/i18n/en.js`, `F/` = `crates/falcon-app/src/`, `FL/` = `crates/falcon-library/src/`.

### 0. Where each viewer opens (the key layout finding)

In Eagle, every viewer opens in the same place. Detail mode is a single `content-panel detail-mode` whose left edge is the sidebar width and whose right edge is the inspector width (`E/index.html:1007`), and every viewer type sits inside it as an `ng-switch` branch (`E/index.html:1268-1545`). So images, GIFs, RAW, TIFF, TGA, SVG, video, audio, fonts, text, PDF, 3D models, special formats, web pages and plugin viewers **all open inside the main window with the sidebar and inspector still visible**. A toggle button in the viewer toolbar (`toggle-all-btn`, `appmenu.view>toggleAll`) hides or shows both panels. Separately, "Open in New Window" (`Context.Open.New.Window`, `view.file.opennewwindow` = Cmd+O, middle click by default) opens a standalone preview window (`E/preview-window.html`), and Space opens a lighter "inline" quick preview (`keyspace: preview`, `E/app.bundle.js:33595`).

Falcon now does the same: `fn viewer` draws every viewer in the central panel, replacing the toolbar, filter bar and grid as Eagle's detail mode does (`index.html:554` hides Eagle's toolbar in detail mode).

| Viewer | Eagle placement | Falcon placement | Falcon status |
| --- | --- | --- | --- |
| Image (jpg/png/webp/bmp/avif/heic…) | Inside main window, sidebar and inspector kept (`index.html:1007`, `:1515`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`) | Done |
| GIF | Inside main window (`index.html:1306`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`) | Done |
| RAW | Inside main window (`index.html:1327`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`) | Done |
| TIFF / TGA | Inside main window (`index.html:1398`, `:1419`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`) | Done |
| SVG | Inside main window (`index.html:1496`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`) | Done |
| Video | Inside main window (`index.html:1343`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`) | Done |
| Audio | Inside main window (`index.html:1366`, `:1377`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`) | Done |
| Font | Inside main window (`index.html:1388`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`) | Done |
| Text (.txt) | Inside main window (`index.html:1393`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`), as a still render | Partial |
| PDF | Inside main window (`index.html:1301`) | Central area, sidebar and inspector kept (`F/app.rs:3392`, `F/app/pdf_view.rs:958`) | Done (macOS only) |
| 3D model | Inside main window (`index.html:1443`) | Central panel, interactive for obj/stl/ply/glb/gltf/3mf, a still for fbx/3ds/dae/ifc | Partial |
| Special formats (PSD, AI, Office…) | Inside main window (`index.html:1448`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`), as a QuickLook render | Partial |
| Web page (url/html/mhtml) | Inside main window (`index.html:1491`) | Central panel; live WKWebView for url/html on macOS, MHTML as a still | Partial |
| Toggle sidebar and inspector from inside the viewer | `toggle-all-btn` in the detail toolbar (`index.html:1015`) | Panels button in every viewer bar and the main toolbar (`App::panels_button`, `logic::toggle_all`), ⌥⌘3 and Tab; both-on/both-off like Eagle | Done |
| Separate preview window | `preview-window.html`, `Context.Open.New.Window` | No second viewer window (only the collect window uses a viewport, `F/collect.rs:84`) | Missing |
| Space quick preview (inline mode) | Inline overlay with prev/next arrows, counter and exit button (`index.html:1242-1254`, `app.bundle.js:33612`) | Space opens the same full viewer (`F/app.rs:1092`) | Partial |

### 1. Detail viewer shell (shared by all viewer types)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Open an item by double-click or Enter | Done | `F/app.rs:2545` | local |
| Preference: double-click opens in the default app instead | Done | `Prefs::double_click` (Controls pane) | local |
| Exit with Esc or the back button | Done | `F/app.rs:1077`, back button `F/app.rs:3481` | local |
| Exit by double-clicking the viewer background | Done | Double-click anywhere in the image viewer closes it, except in crop or comment mode, as Eagle's `dblclickContentPanel`; video, audio, PDF and font don't, as in Eagle | local |
| Position counter "n / total" in the toolbar | Done | `logic::viewer_counter`, drawn after the back button in `media_view::top_bar` | local |
| Previous and next buttons in the toolbar | Done | Bar buttons and arrows share `App::viewer_step`; disabled at the ends | local |
| Previous and next with the arrow keys | Done | `F/app.rs:1110-1135` | local |
| "Already the first item" and "Already the last item" tips | Done | Status note over the viewer for about 1.2 s (`logic::Edge`) | local |
| Mouse gesture: right-drag left or right moves to the previous or next item | Missing | None | local |
| Mouse gesture: right-drag up or down zooms | Missing | None | local |
| Right-click in the viewer opens the item context menu | Done | `App::item_menu` reused by the image viewer and the audio/font background; video and PDF have none (Eagle's PDF is an iframe, a click on video toggles playback) | local |
| Middle-click in the viewer opens the plugin panel | Missing | No plugins | online/plugin |
| Pinned plugins and plugin button in the viewer toolbar | Missing | No plugins | online/plugin |
| Open in default app / Open With from the viewer | Done | `F/app/open_ui.rs:109`, used at `F/app.rs:3486` and `F/app/media_view.rs:79` | local |
| "Cannot preview" page with an "Open With Other Application" button | Done | Icon, name, a line of text and an Open With Another App button; the page's menu has Open, Open With and Show in Finder; shown only when neither render nor thumbnail exists | local |
| "Image too large to preview" page (TIFF/HEIC/TGA above the pixel limit) | Done | `logic::too_large_to_preview` (Eagle's 120,000,000 px limit for tif/tiff/heic/heif/hif/tga without a custom thumbnail); `image_view.rs` `too_large` with the thumbnail and an Open With button | local |
| Unsupported format suggests a plugin from the plugin center | Missing | No plugins | online/plugin |
| Presentation (slideshow) mode: full screen, panels hidden | Done | F5, View menu, item menu, middle click; Esc leaves and keeps the viewer | local |
| Always on top | Done | ⇧T (`modes.rs`) | local |
| Thumbnail background per item (none, black, gray, white, grid) used in the viewer and grid | Done | `paint_thumb_bg` from the item's `background` key; wins over the transparency-grid preference | local |
| Transparent-area display preference (grid or no grid) | Done | `prefs.transparency_grid`, off by default as Eagle | local |
| Grayscale preview toggle | Done | Image, GIF, video, PDF, font, grid and the preview window; ⌥⌘G, View menu and item menu | local |
| Navigator (minimap) while zoomed | Done | Eagle's 120 px box, 16 px inset, shown by its overflow rule; click centres, drag pans (`logic::navigator_*`); ⌥⌘8 and View ▸ Navigator | local |
| Viewer keys follow the shortcut table (zoom, crop, rotate, flip) | Done | `view.zoom.*`, `edit.image.crop`, `edit.image.rotate` ⇧R, `edit.image.flip` ⇧F (Eagle's ids); the keys act only while a viewer is open | local |

### 2. Image viewer (jpg, png, webp, bmp, avif, heic, jxl, ico and the like)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Fit to window on open ("Smart Zoom") | Done | `logic::smart_fit` | local |
| Preference: open at original size instead | Done | `prefs.open_actual_size` | local |
| Long images (very tall or wide) open fitted to width for scrolling | Done | `logic::smart_fit` with Eagle's thresholds (width ≥ 960 and height > 1.8×, or width ≥ 320 and height > 2.7×); tall images only, as Eagle has no rule for wide strips | local |
| Zoom slider (5–200%) in the toolbar | Done | `image_view.rs`: positions 5–200 showing 5–800% (7% per step above 100), snapping as Eagle | local |
| Zoom presets menu (5%–800%, Actual Size, Zoom to Fit) from the % label | Done | `image_view.rs` size menu on the % label; right-click toggles actual size and fit | local |
| Zoom in and out buttons and keys | Done | ⌘= and ⌘− act on the viewer while it is open (`logic::view_zoom`); unmodified + and − still work | local |
| Actual size (100%) | Done | "1:1" button, the 1 key and ⌘0 (`view.zoom.actual`) | local |
| Zoom to fit | Done | "Fit" button, the 0 key and ⌘9 (`view.zoom.fit`) | local |
| Fit to window edge toggle (backtick key) | Done | Backtick, saved in `Saved.zoom_edge` | local |
| Pinch zoom around the pointer | Done | `F/app/image_view.rs`, 5–800% | local |
| Mouse-wheel behaviour preference (scroll, zoom, or previous/next) | Done | `logic::wheel_action`, `prefs.wheel`: scroll (default), zoom or page; no first-use prompt | local |
| Pan by dragging | Done | `F/app/image_view.rs` | local |
| Remember the last zoom and position per item | Done | `prefs.remember_zoom`, per session as Eagle's `lastItemStates` | local |
| Pixelated or smooth rendering when zoomed in | Partial | `prefs.pixelated`: nearest sampling above 100% of screen pixels (Eagle starts at 200%); grid thumbnails stay smooth | local |
| Shows the original at full resolution | Done | `Tier::Original` in `loader.rs`: the original decoded on a worker and cut into ≤ 2048 px tiles within the GPU limit, kept only for the viewed item (`keep_original`); jpg, png, webp, bmp, tiff and RAW previews | local |
| EXIF orientation applied in the viewer | Done | `FL/thumbnail.rs:121-130` | local |
| Rotate | Done | `F/crop.rs:242-250`, Shift+R `F/image_edit.rs:281` | local |
| Rotation mode preference (preview only, or write to file) | Done | `imageRotateMode` preference, default Write; preview mode only changes how the viewer draws | local |
| Horizontal flip (Shift+F), vertical flip | Done | `F/crop.rs:251-258`, Shift-click flips vertically (`F/image_edit.rs:332`) | local |
| Rotate and flip limited to jpg/png/webp/bmp (flip also avif) | Done | `FL/transform.rs:36-53` | local |
| Crop tool (jpg/png/webp) with handles | Done | Crop with handles, ⇧C, W/H fields, Save / Save As / Cancel (`crop.rs`); in-place writes keep a `.bk` until synced, as Eagle | local |
| Crop width and height number fields | Done | `crop.rs` bar fields | local |
| Crop "Save As" (keeps the original, adds a new item) | Done | `crop_item_as_new`: same name, tags, folders, URL and rating, dated `modificationTime + 1` (Eagle +0.1); note not carried | local |
| Live crop size readout | Done | `x: y: W: H:` shown while dragging | local |
| Comment mode: draw rectangle regions with notes (C key) | Done | `F/comments.rs:201-320`, C key `F/image_edit.rs:277` | local |
| Comment badges, delete X, resize handles, count badge on the toolbar button | Done | `F/comments.rs:213-318` | local |
| Hovering a region shows its note | Done | Tooltip on hover (`F/comments.rs:290`) | local |
| Comment count badge on grid cards | Done | `F/comments.rs:180` (always on; no toggle) | local |
| Default viewer mode preference (preview or comment) | Missing | Eagle declares `habits.defaultMode` but never reads it; not applicable | local |
| Copy, copy path and copy link from the viewer menu | Done | The viewer's right-click menu is the item menu: Copy (files), Copy File Path, Copy ▸ Link (Eagle's local API link) / Eagle Link / Source URL | local |

### 3. GIF and animated images

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| GIF plays in the viewer | Done | `F/anim.rs` streams every frame from disk on a background thread with a bounded frame window (66 MB peak on a 5,000-frame GIF) | local |
| Animated WebP, AVIF and APNG play in the viewer | Partial | WebP and APNG through `anim.rs`; animated AVIF through ImageIO (`anim::Kind::ImageIo`) on macOS only | local |
| Frame-by-frame GIF player toggle | Done | `F/app/gif_view.rs` `gif_toggle_button`; `prefs.gif_player` (off by default, as Eagle's `habits.gifViewer`) | local |
| Play and pause (Space) | Done | `gif_view.rs` `gif_keys`, read before the viewer's keys so Space doesn't close it while the player shows | local |
| Previous and next frame ([ and ]) | Done | The rebindable Player keys; ⇧[ / ⇧] step ten frames | local |
| Frame counter (current / total) | Done | Padded counter in the player bar | local |
| Scrub through frames on a progress bar, with a frame thumbnail on hover | Done | `gif_view.rs`; scrolling over the bar steps frames | local |
| Playback speed 0.25x–8x | Done | Eagle's eleven steps; ⇧. / ⇧, also change it | local |
| Loop range (A-B) set on the progress bar, and reset | Done | Drag handles at both ends of the bar; Reset in the more menu | local |
| Set current frame as the GIF's thumbnail | Done | `FL/thumbnail.rs` `set_frame_thumbnail` writes `<name>_thumbnail.png` (short side ≤ 480 px) and recomputes the palette, no `customThumbnail` key, as Eagle | local |
| Comments on GIFs | Done | `F/comments.rs:169` allows GIFs | local |

### 4. RAW photos (arw, cr2, cr3, nef, dng, raf, orf, rw2 and others)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| RAW viewer shows the thumbnail, then the embedded full preview | Done | `thumbnail::raw_preview` extracts the largest embedded baseline or progressive JPEG, turned by EXIF orientation; QuickLook fallback on macOS | local |
| EXIF orientation for RAW | Partial | Whatever QuickLook applies | local |
| Camera metadata (camera, ISO, focal length, shutter, aperture, date) | Done | `F/app/media_view.rs:315-333` (when `rawMetas` exists) | local |
| Zoom, comments on RAW | Done | Same image viewer and `F/comments.rs:169` | local |

### 5. Video viewer

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Play native formats (mp4, mov, m4v and others) | Done | AVFoundation player (`F/app/video_view.rs:114-145`) | local |
| Fallback player for formats Chromium can't play (mkv, avi, wmv, flv and others) | Missing | No ffmpeg or mpv; the can't-play page names the format (`F/app/video_view.rs` `cannot_play`) | local |
| "Can't play this file" page with an Open With button | Done | `video_view.rs` `cannot_play`, `cannot_play_line`: thumbnail, name, a line naming the format, Open With | local |
| Autoplay on open | Done | `F/app/video_view.rs`; switch `Prefs::video_autoplay` | local |
| Play/pause button, click picture to toggle, Space | Done | `F/app/video_view.rs:309-312`, `:385-392`, `F/shortcuts.rs:122` | local |
| Seek bar with drag scrubbing | Done | `F/app/video_view.rs:320-349` | local |
| Hover thumbnail on the seek bar (preview frame under the pointer) | Done | `SeekPreview` fed by `falcon_video::Grabber` on a worker thread | local |
| Skip back and forward buttons (2%, capped at 10 s / 20 s), hold to repeat | Done | `Repeat`: acts on press, repeats every 100 ms while held; 2% capped at 10 s back / 20 s forward | local |
| Frame step back and forward, 1 and 10 frames | Done | Buttons and [ ] Shift+[ Shift+] (`F/app/video_view.rs:402-408`, `F/shortcuts.rs:123-126`) | local |
| Playback speed menu 0.25x–8x and Shift+. / Shift+, | Done | `F/app/video_view.rs:441-452`, `F/shortcuts.rs:131-132` | local |
| Loop toggle, remembered | Done | `F/app/video_view.rs:431-439` | local |
| Always loop clips shorter than 30 s | Done | `F/app/video_view.rs`; switch `Prefs::video_loop_short` | local |
| Volume slider, mute, volume keys (Cmd+Up/Down), remembered | Done | `F/app/video_view.rs:412-430`, `F/shortcuts.rs:127-128` | local |
| Volume / seek tip overlay while adjusting | Done | `volume_tip`, top right for 1 s; Eagle has no seek tip either | local |
| Wheel over the picture seeks; Shift/Alt+wheel seeks horizontally | Done | `F/app/video_view.rs`; `Prefs::video_scroll` can make the wheel change the volume instead | local |
| Preference: vertical wheel controls volume instead | Done | `VideoScroll::Volume` preference, applied in `ui/viewer/video.rs` | local |
| Remember the last playback position | Done | `Prefs::video_resume`; positions saved per item id in app state (`Positions`, latest 500), across launches | local |
| Stretch video to fill the window | Done | `Prefs::video_fill` | local |
| Full-screen button | Done | Enters presentation mode, as Eagle's does | local |
| Flip and rotate the video (viewer toolbar) | Done | `rotate_step`, `corner_uvs`; view only, also ⇧R / ⇧F (⇧-click rotates clockwise) | local |
| Timed notes: add a note at the current time | Done | `F/app/video_view.rs:453-458`, N key `F/shortcuts.rs:134` | local |
| Timed notes shown as markers on the seek bar; click to jump, edit, delete | Done | `F/app/video_view.rs:351-381` | local |
| Set current frame as the video thumbnail | Done | `Library::set_video_thumbnail`: `thumbnailAt` kept in metadata, 480 px JPEG under `<name>_thumbnail.png`, palette recomputed, as Eagle | local |
| Copy current frame to the clipboard | Done | `frame_tool`, ⇧C (full decoded frame; Eagle copies at the recorded size) | local |
| Save current frame as an image | Done | ⇧S imports a `<name> - <t>` JPEG as a new item with the video's URL and folders, as Eagle | local |
| Load subtitles (.srt, .vtt) | Done | `F/subtitles.rs` parser (no crate); sidecar `<name>.vtt/.srt` in the `.info` folder, Load Subtitles… copies one in, as Eagle | local |
| Open in default app from the video toolbar | Done | `F/app/media_view.rs:79` | local |

### 6. Audio viewer

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Waveform display | Done | `F/app/media_view.rs:114-136` | local |
| Click or drag the waveform to seek | Done | `F/app/media_view.rs:137-148` | local |
| Autoplay on open | Done | `F/app/media_view.rs:107` | local |
| Play/pause button and Space | Done | `F/app/media_view.rs:151-160`, `F/app.rs:1084` | local |
| Time readout | Done | `F/app/media_view.rs:161` | local |
| Loop toggle | Done | `F/app/media_view.rs:172-181` | local |
| Volume slider | Done | `F/app/media_view.rs:163-170` | local |
| Volume keys (Cmd+Up/Down) | Done | `video_view.rs` `audio_action`: the Player key table drives the audio player | local |
| Skip back and forward keys (Cmd+Left/Right) | Done | `audio_action` | local |
| Playback speed (menu and Shift+. / Shift+,) | Done | `Audio::set_speed`, `MENU_RATES` 0.5–2× in the menu, 0.25–8× by key; pitch follows speed | local |
| m4a playback | Done | `F/audio.rs:48` via rodio mp4 | local |
| BPM and duration in the inspector | Done | `F/app/media_view.rs:306-313` | local |

### 7. Font viewer

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Specimen drawn in the font itself | Done | `F/app/media_view.rs:241-298` | local |
| Editable sample text | Done | `F/app/media_view.rs:262-275` | local |
| Size slider with zoom-in and zoom-out buttons | Done | `F/app/font_view.rs`: 50–1000% slider with − and +, ⌘=/⌘−/⌘0/⌘9, modifier-wheel and pinch | local |
| Tabs: Article, Waterfall, Alphabet, Other (info) | Done | `font_view.rs` tabs Article / Waterfall / Alphabet / Info, remembered in app state | local |
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
| View text in the app | Done | `app/text_view.rs` in the central viewer panel, `textdoc.rs` rules; non-UTF-8 or files over 2 MB open read-only | local |
| Edit the text in place, auto-saved | Done | `Library::save_text` (`crates/falcon-library/src/text.rs`) writes atomically 500 ms after typing stops, on blur, ⌘S and leaving; refreshes `size`, `mtime`, `lastModified`; empty text is never saved, as Eagle | local |
| Edit the title (rename) at the top of the page | Partial | `Action::RenameItem` on blur/Enter; Eagle 4.0.0 ships this commented out, so it is a documented difference | local |
| Zoom the text size (wheel and buttons) | Done | Eagle's five sizes 14–42 pt; ⌘=/⌘−, ⌘0/⌘9, ⌘-wheel/pinch and bar buttons; size kept in `Saved.text_zoom` | local |
| Reloads when the file changes outside | Done | One `stat` a second, reload on an mtime change unless typing is pending; F5 reloads | local |
| Links in the text open in the browser | Done | http(s)/www. links underlined, ⌘-click opens through `open_with::open_url`; Eagle's handler never fires | local |
| Number keys rate the item from inside the editor | Done | 0–5 when neither the text nor the title has focus | local |

### 9. PDF reader (pdf.js)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Opens inside the main window with sidebar and inspector visible | Done | `F/app.rs:3392`, `F/app/pdf_view.rs:958-1011` | local |
| Works on Windows and Linux too | Missing | macOS only; elsewhere the image viewer shows a still (`F/app/pdf_view.rs:959`) | local |
| Continuous vertical page scrolling | Done | `F/app/pdf_view.rs:1015-1121` | local |
| Previous and next page buttons, n/j/p/k keys | Done | `F/app/pdf_view.rs:1242-1248`, `:405-406` | local |
| Page number field and page count | Done | `F/app/pdf_view.rs:1222-1241` | local |
| First and last page (Home, End) | Done | `F/app/pdf_view.rs:407-408` | local |
| Zoom in and out buttons and Cmd+= / Cmd+- / Cmd+0 | Done | `F/app/pdf_view.rs:1199-1219`, `:393-395` | local |
| Zoom menu: Automatic, Actual Size, Page Fit, Page Width, 50%–400% | Done | `F/app/pdf_view.rs:1205-1216` | local |
| Pinch or Cmd+wheel zoom around the pointer | Done | `F/app/pdf_view.rs:1016-1023` | local |
| Sidebar with page thumbnails | Done | `F/app/pdf_view.rs:1132-1163` | local |
| Sidebar outline (bookmarks) | Done | `F/app/pdf_view.rs:1164-1192` | local |
| Highlight the outline entry for the current page | Done | `pdf_view.rs` outline follows the page and scrolls to it | local |
| Sidebar attachments list | Done | `falcon_pdf::Document::attachments` through CGPDF (EmbeddedFiles tree and file annotations); click saves the file | local |
| Sidebar layers (optional content) | Partial | Listed with their default visibility (`Document::layers`); not switchable, PDFKit draws only the default configuration | local |
| Find bar with previous/next, match count | Done | `F/app/pdf_view.rs:1286-1332` | local |
| Find option: match case | Done | `F/app/pdf_view.rs:1310-1312` | local |
| Find option: highlight all | Done | All hits always drawn (`F/app/pdf_view.rs:1059-1064`); no toggle | local |
| Find options: whole words, match diacritics | Done | `falcon_pdf::is_whole_word`, find options menu | local |
| Select text and copy it | Done | `F/app/pdf_view.rs:1080-1119`, copy `:946` | local |
| Hand tool (drag to pan) | Done | Tools menu; keys s / h as pdf.js | local |
| Rotate pages clockwise and counter-clockwise | Done | r / ⇧R, view only; find hits and selections follow | local |
| Scroll modes: page, vertical, horizontal, wrapped | Done | Tools menu; page mode turns with n/p or the wheel at the page end; remembered per document | local |
| Spread modes: none, odd, even (two-page) | Done | Tools menu; Page Fit and Page Width fit the spread | local |
| Presentation mode (full screen) | Done | Tools ▸ Presentation Mode uses the app's presentation mode | local |
| Print | Done | ⌘P / Tools ▸ Print… through `falcon_pdf::print` (`NSPrintOperation`, main thread, re-opened with the document's password) | local |
| Document properties dialog | Done | Tools ▸ Document Properties: file, size, metadata, dates, version, pages, page size with paper names, fast web view, password | local |
| Password-protected PDFs | Done | `Document::open_with_password` (`unlockWithPassword`); the worker asks the UI and retries | local |
| Clickable internal and external links | Done | `Document::links` from PDFKit annotations: GoTo and URL actions; only http, https and mailto open | local |
| Remember page and zoom per document | Done | `F/app/pdf_view.rs:343-358`, `:866-891` | local |
| ←/→, Space and Esc still move between items or close | Done | `F/app/pdf_view.rs:377-381` | local |
| Download, Open File, draw and free-text editing | n/a | Hidden in Eagle too | local |

### 10. 3D model viewer (Online 3D Viewer: fbx, obj, 3ds, 3mf, dae, ifc, ply, stl, glb)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Interactive 3D view (orbit, pan, zoom) | Partial | `falcon_library::model3d` readers (obj+mtl, stl, ply, glb/gltf, 3mf) and an `egui_wgpu` callback viewer (`ui/viewer/model.rs`): Eagle's camera, mouse mapping, lighting, edges, details; grid/axes extras; thumbnails rendered at import. fbx, 3ds, dae and ifc stay QuickLook stills; no mesh tree, export, snapshot or textures | local |
| Fit model to window | Done | Fit button (⌘0), `viewer3d::fit` | local |
| Up-axis (Y/Z), flip up vector, fixed or free orbit | Done | Settings menu: Y/Z up, Flip, Fixed Up Vector / Free Orbit | local |
| Perspective or orthographic camera | Done | Settings menu: Perspective / Orthographic | local |
| Measure tool | Missing | None | local |
| Model tree navigator (files, meshes, materials) and details sidebar | Partial | Details panel with materials; no tree of files or meshes | local |
| Settings: background colour, edge display, dark theme | Partial | Background colour, edges with colour and angle, default colour, Reset; no dark-theme setting | local |
| Export model and create snapshot | Missing | None | local |
| Esc/←/→ still exit or move between items | Done | The shared viewer shell handles Esc, ← and → | local |

### 11. Special formats viewed as images (PSD, PSB, AI, EPS, XD, Sketch, Figma, Affinity, C4D, Blender, SketchUp, DWG, CDR, Clip, InDesign, Office, iWork, XMind, MindNode, OmniGraffle, HDR, EXR, ICO, ICNS, DDS)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Shown as a zoomable image of the thumbnail | Done | Image viewer falls back to the preview tier (`F/app.rs:3427-3429`) | local |
| Higher-resolution render made on demand for PSD, PSB, AI, PPT, PPTX, POTX | Partial | A QuickLook render up to 3072 px for every unsupported format (`F/loader.rs:56-57`, `:209-211`); macOS only | local |
| Comments on special formats | Done | `F/comments.rs:169` | local |
| Custom thumbnail shown in the viewer for formats with no preview | Partial | Full tier tries QuickLook on the original first; the custom thumbnail only shows if that fails (`F/loader.rs:56`, `F/app.rs:3427`) | local |
| HDR/EXR rendered with tone mapping and exposure | Partial | QuickLook render, no exposure | local |

### 12. SVG, TIFF, TGA, HEIC

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| SVG drawn as vectors, sharp at any zoom | Done | `thumbnail::render_svg_region` re-renders the visible part at screen resolution 0.15 s after the view holds still (`logic::svg_region`) | local |
| TIFF viewer (multi-page aware, worker decode) | Done | `tiff_pages.rs`: pages counted on a worker, `‹ 2 / 5 ›` in the bar, PageUp/PageDown; Eagle's own TIFF viewer decodes only the first page | local |
| TGA viewer | Partial | QuickLook still | local |
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
| Open one or more items in their own window (Cmd+O, middle click) | Partial | ⌘O and middle click (preference) open one item per window; no multi-item opening | local |
| Same viewers in the window (image, GIF, RAW, video, font, text, PDF, 3D, web, plugin) | Partial | Image, GIF, video, audio, text and PDF are real viewers in the window; fonts, 3D and web show the thumbnail | local |
| Window opacity 10%–100% | Done | NSWindow `alphaValue` from the window menu | local |
| Always on top toggle in the window | Done | Preview window menu | local |
| Navigator, grayscale, rotate, flip in the window menu | Done | Preview window menu; rotate and flip change the preview only | local |
| Copy, copy path, copy link, reveal in Finder, open in default app | Done | Copy, Copy File Path, Copy ▸ Link / Eagle Link, Reveal in Finder, Open in Default App | local |
| Copy or save the current video frame | Done | Preview window video player: Copy Frame, Save Frame… | local |
| Reveal in Eagle (select the item in the main window) | Done | Show in Falcon | local |

### 16. Grid hover behaviours

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Hover zoom button on cards, enlarged preview after 200 ms | Done | `F/hover.rs:10`, `F/app.rs:2680-2743` | local |
| Hold Z over a card for an instant preview | Done | `F/app.rs:2702-2704`, `F/hover.rs:397` | local |
| Preview loads the original for raster formats | Done | `F/hover.rs:26`, `F/app.rs:2730-2736` | local |
| Animated GIF plays inside the hover preview | Done | `gif_view.rs` `hover_anim`, `anim.rs` `Cards::popup` | local |
| GIF and animated images play when hovered in the grid | Done | GIF, animated WebP and (macOS) animated AVIF (`card_anim`, `Cards::play`) | local |
| Preference: always autoplay GIF/WebP/AVIF in the grid | Done | `prefs.always_play_animated` (off by default, as Eagle); AVIF on macOS | local |
| Video hover preview: autoplays, moving the pointer scrubs, progress bar and time | Partial | 48-picture strip from `falcon_video::Grabber` on a worker: 500 ms delay, scrub, resume after 200 ms rest, bar and time; no sound (`Prefs::video_hover`, on by default) | local |
| Mute toggle on the video hover preview, remembered | Missing | None | local |
| Audio hover preview: plays, click to seek, progress | Partial | Separate hover player after 200 ms, progress tint, click to seek, remembered play switch; doesn't stop when the window hides | local |
| YouTube, Vimeo and Bilibili hover previews for bookmarked links | Missing | None | online/plugin |
| Text files show their text on the card | Missing | Extension placeholder (`F/app.rs:2593`) | local |

### 17. File formats

Eagle columns come from `config.js` (`SUPPORT_FORMATS_DARWIN`), `E/js/thumbs/thumb.js:94-309` and the viewer switch in `E/index.html`. Falcon columns come from `FL/thumbnail.rs:42-44` (formats falcon decodes itself: png, jpg, jpeg, webp, gif, bmp, svg), `FL/import.rs:296-351` (drawn audio and font thumbnails, QuickLook for the rest), and `F/loader.rs:52-66` (viewer source). "QL" means macOS QuickLook: it works only on macOS and only where macOS has a QuickLook generator for that type, so results vary by machine.

| Format | Eagle viewer | Falcon thumbnail | Falcon viewer |
| --- | --- | --- | --- |
| jpg, jpeg, jfif, jpe, insp | Image viewer, full res | Decoded | Image viewer (cap 3072 px) |
| png | Image viewer | Decoded | Image viewer (APNG animates) |
| webp | Image viewer, animates | Decoded (first frame) | Image viewer, animates |
| gif | GIF viewer with frame player | Decoded (first frame) | Animated, all frames, frame player |
| bmp | Image viewer | Decoded | Image viewer |
| svg | Vector image viewer | Rendered (resvg) | Rasterised at up to 3072 px |
| avif | Image viewer, animates | QL | Image viewer, full res, animates (macOS ImageIO) |
| jxl | Image viewer | QL (macOS 14+) | Image viewer, full res (ImageIO, macOS 14+) |
| heic, heif, hif | Image viewer | QL | Image viewer, full res (ImageIO, macOS) |
| tif, tiff | TIFF viewer | QL | QL still |
| tga | TGA viewer | QL | QL still |
| ico | Image of thumbnail | QL | QL still |
| icns | Image of thumbnail | QL | QL still |
| dds | Image of thumbnail | QL | QL still |
| hdr | Image of thumbnail | QL | QL still |
| exr | Image of thumbnail | QL | QL still |
| RAW: 3fr arw cr2 cr3 crw dng erf mrw nef nrw orf pef raf raw rw2 sr2 srw x3f | RAW viewer (embedded full preview) | QL | QL still |
| psd, psdt, psb | Native high-res viewer | QL | QL still |
| ai, ait | Native high-res viewer | QL | QL still |
| eps | Image of thumbnail | QL | QL still |
| xd | Image of thumbnail | QL if available | QL still |
| sketch | Image of thumbnail | QL if available | QL still |
| fig | Image of thumbnail | QL if available | QL still |
| af, afdesign, afphoto, afpub | Image of thumbnail | QL if available | QL still |
| c4d, blend, skp, dwg, cdr, clip, prd, skt | Image of thumbnail | QL if available | QL still |
| pxd, graffle, mindnode | Image of thumbnail | QL if available | QL still |
| xmind | Image of thumbnail | QL if available | QL still |
| eddx, emmx | Image of thumbnail | QL if available | QL still |
| indd, indt, idml | Image of thumbnail | QL if available | QL still |
| doc, docx | Image of thumbnail | QL | QL still |
| xls, xlsx | Image of thumbnail | QL | QL still |
| ppt, pptx, potx | Native high-res viewer | QL | QL still |
| key, numbers, pages | Image of thumbnail | QL | QL still |
| pdf | pdf.js reader in main window | QL | PDFKit reader in main window (macOS) |
| txt | Editable text editor | None | QL still |
| ttf, otf, ttc, woff, woff2, otc | Font viewer | Drawn specimen 600×600 | Font viewer (falls back if egui can't read the font) |
| mp3, wav, ogg, flac, aac, m4a | Audio player with waveform | Drawn waveform 280×140 | Audio player with waveform |
| mp4, mov, m4v, 3gp, ts, mts, m2ts | HTML5 video player, mpv fallback | QL | AVFoundation player |
| webm, mkv, avi, wmv, mpg, flv, f4v | HTML5 player or mpv fallback | QL if available | AVFoundation if it can, else a still |
| 360, afx, eva, vap | Video player | QL if available | AVFoundation if it can, else a still |
| glb, obj, fbx, 3ds, 3mf, dae, ifc, ply, stl | Interactive 3D viewer | QL if available (obj/stl/ply/dae usually; fbx/ifc/glb rarely) | QL still |
| url | Live web view | QL (generic) | Live WKWebView (macOS); `.url`/`.webloc` addresses read on import |
| html, mhtml | Live web view | QL | QL still |

### 18. Ranked list of local Missing and Partial items

Ranked by how often a user would notice them.

1. **Viewers cover the whole window (Missing).** In Eagle, every viewer opens between the sidebar and inspector (`index.html:1007`). Falcon does this only for PDFs (`F/app.rs:3392`). This affects image, GIF, video, audio, font, text, 3D and special-format viewing.
2. **No toggle for sidebar and inspector inside the viewer (Missing)** (`toggle-all-btn`).
3. **Image resolution capped at 3072 px (Partial).** Zooming into large photos and SVGs is soft (`F/loader.rs:26`).
4. ~~No GIF frame player.~~ Fixed.
5. ~~Animated AVIF doesn't animate.~~ Fixed on macOS through ImageIO; WebP and APNG everywhere.
6. **No video hover-scrub preview in the grid (Missing).** Also missing: audio hover playback. GIF and WebP cards do play on hover.
7. **No video frame tools (Missing).** You can't set a frame as the thumbnail, copy it or save it.
8. **No fallback player for mkv, avi, wmv, flv or webm (Missing).**
9. **No interactive 3D model viewer (Missing).**
10. **No text editor for .txt (Missing).** No editing, renaming, zoom or auto-reload, and cards don't show the text.
11. **No viewer context menu or right-drag gestures (Missing).** Right-click menu, swipe to previous or next, drag to zoom.
12. **No zoom slider or zoom presets menu, and different zoom keys (Partial).** Cmd+=/Cmd+-/Cmd+0/Cmd+9 and backtick don't work in the viewer.
13. **No presentation mode or full screen (Missing).** Also affects the video full-screen button and PDF presentation mode.
14. **No separate preview window (Missing).** No opacity, always on top or reveal in Eagle.
15. **No viewer background options (Missing).** No per-item background, transparency grid preference or pixelated zoom.
16. **No navigator minimap or grayscale preview (Missing).**
17. ~~Viewer preferences are absent.~~ Fixed: wheel behaviour, default size, pixelated zoom, remembered zoom, transparency grid, GIF player and grid autoplay.
18. **Remember last zoom and position per item, and persist video position across launches (Missing / Partial).**
19. **PDF gaps (Missing).** No page rotation, hand tool, scroll and spread modes, presentation, print, document properties, password prompt, links, attachments, layers, current outline item, or whole-word and diacritics search. The reader is also macOS only.
20. **Crop gaps (Missing).** No width and height fields, no Save As, no size readout and no Shift+C.
21. **Audio gaps (Missing).** No speed control, and no volume or skip keys.
22. **Font viewer gaps**: fonts egui can't read (some woff2, CFF) still fall back to the image viewer.
23. **Space opens the full viewer instead of Eagle's inline quick preview (Partial).**
24. **No position counter, prev/next buttons or first/last-item tips in the viewer bar (Missing).**
25. **Video gaps (Missing / Partial).** No seek-bar hover thumbnail, flip and rotate, subtitles, volume and seek tip overlay, or hold-to-repeat skip.
26. **RAW, TIFF, TGA, HEIC, AVIF, JXL and design formats rely on QuickLook (Partial).** This works only on macOS and depends on the machine. There is no RAW embedded-preview extraction and no HDR exposure.
27. **No "cannot preview" page with an Open With button, and no "too large" message (Partial).**
28. **Long images open fitted to the window instead of to the width (Missing).**
29. **No "double-click opens in the default app" preference (Missing).**
30. **Fonts egui can't read fall back to the image viewer (Partial).**

## Search, filters and organising


Scope: search box, filter bar, sorting, folders, smart folders, tags, Quick Access, inspector, batch editing, duplicates, trash, ratings, comments, saved filters and the other organising commands.

Every falcon status below comes from reading falcon's code, not from its ROADMAP or docs.

Path shorthand:
- Eagle: `B` = `app/app/app.bundle.js`, `R` = `app/app/js/rule-match.js`, `D/` = `app/app/js/directives/`, `en` = `i18n/en.js`, `idx` = `app/app/index.html`.
- Falcon: `S` = `crates/falcon-library/src/search.rs`, `SM` = `.../smart.rs`, `M` = `.../model.rs`, `E` = `.../edit.rs`, `A` = `crates/falcon-app/src/app.rs`, `L` = `.../logic.rs`, `O` = `.../organize.rs`, `OU` = `.../app/organize_ui.rs`, `FU` = `.../app/filters_ui.rs`, `DV` = `.../app/duplicates_view.rs`.

Status values: Done = works as in Eagle. Partial = exists but is missing options or behaves differently. Missing = not there.

### 1. Search box

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Type to search the current view, with a short debounce | Done | `A:2061` search_field; `L:144` visible | local |
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
| Exact-substring results only | Partial | Falcon adds its own typo-tolerant fallback (`S:191` fuzzy_score, `L:165`), which returns results Eagle would not. This is a deviation to keep or put behind an option. | local |
| Traditional and simplified Chinese match each other | Missing | none | local |
| Pinyin matching for folder, smart folder and tag names in sidebar and tag lists | Missing | `A:1904` subtree_matches is a plain substring test | local |
| Toolbar title reads "Search results" while searching | Done | `A:2019` | local |
| Empty-result message | Done | `ui/grid/empty.rs`: per-view messages (No Matches, Drop Files Here, Trash Is Empty, All Sorted…) | local |
| Search by image from the search box (AI) | Missing | The camera icon is drawn disabled (`A:2069`) | online/plugin |

### 2. Filter bar (general)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Show or hide the filter bar | Done | `A:2045`, Command::ToggleFilter | local |
| Badge or highlight while filters are active | Done | `A:2048` outlines the funnel | local |
| Clear all filters | Done | `A:2403` Clear | local |
| Add filter (+) menu: choose and pin which filters appear in the bar | Done | `ui/filter_bar/`: Eagle's default pins, + menu, chips reordered by dragging (`SearchPrefs.order`) | local |
| Lock the filter so it survives switching folders. Unlocked, a view change clears the filter and search | Done | `filter_bar.rs` Lock; not kept across launches, as Eagle | local |
| Per-option counts in each filter menu | Done | Tags, folders, type, shape and rating counts (`type_counts`, `shape_counts`); counted over the whole library | local |
| A shortcut for each filter (open the colour, tag, folder... chip) | Done | ⌥F/⌥T/⌥C/⌥S/⌥R/⌥D/⌥E (`find.filter.*`); the others have unbound rows | local |

### 3. Individual filters

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Colour: preset swatches, any colour, accuracy slider, results sorted by colour distance | Done | `A:2237`, `S:870` ColorQuery, `L:173` distance sort | local |
| Colour: grayscale only | Done | `A:2276`, `S:922` | local |
| Colour: full picker widget | Partial | Hex text field plus 14 swatches, no picker | local |
| Tags: include tags, with Any, All or Equals logic | Done | `A:2289`, `S:362` SetFilter (keeps the OR quirk) | local |
| Tags: Untagged option | Done | `A:2294` | local |
| Tags: exclude a tag (right-click) | Done | `filter_bar.rs` Tags chip | local |
| Tags: search box, Select all, and a sidebar with All, Selected and tag groups; filter by a whole group | Done | Tags chip search, sidebar (All / Selected / groups) and Select all per group | local |
| Folders: include folders with Any, All or Equals; Uncategorized option | Done | `A:2310` | local |
| Folders: exclude, search box, tree view | Done | Folders chip in tree order with counts, search and exclude, Uncategorized included | local |
| Shape: horizontal, vertical, square, panoramic horizontal, panoramic vertical | Done | `A:2331`, `S:433` | local |
| Shape: 4:3, 3:4, 16:9, 9:16 and a custom W:H ratio | Done | `Filter::custom_shape`; exact float equality as Eagle; saved-filter keys `43`/`34`/`169`/`916`/`custom` | local |
| Rating: 5 to 1 stars plus None, OR-ed | Done | `A:2352` | local |
| Type: pick extensions | Done | `A:2372` | local |
| Type: built-in groups (font, video, audio, YouTube, Vimeo, Bilibili), exclude by right-click, search box, counts | Done | `type_options`, Eagle's include order and quirks (video/audio decide alone); falcon counts font/video/audio so they appear, which Eagle's zero counts never do | local |
| Date Added: Today, Yesterday, last 7/30/90/365 days, custom range, months | Done | `FU:68` | local |
| Date Modified: same options | Done | `FU:68` (Modified tab). Falcon also has a Created tab, which Eagle lacks. | local |
| File size min/max in KB or MB | Done | Size chip | local |
| Dimensions: min/max width and height | Done | Dimensions chip | local |
| Duration min/max with s, m or h | Done | `FU:132` | local |
| BPM min/max | Done | `FU:172` | local |
| Note: has, none, or keywords | Done | `A:2393` | local |
| URL: has, none, or keywords | Done | `A:2398` | local |
| Comment (region and video comments): has, none, or keywords | Done | `Filter::comments`; saved under the `annotation` key | local |
| Camera: pick from the cameras present | Done | `FU:201` | local |
| Font: activated or deactivated | Done | `FU:228` | local |
| Visual Match (find by image, drop or paste an image) | Missing | AI search service | online/plugin |
| Semantic search ("black cat") | Missing | AI search service | online/plugin |

### 4. Sorting

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Global sort by Date Added, Created, Modified, Title, Extension, File Size, Dimensions, Rating, Tags, with ascending/descending | Done | `S:1012` OrderBy, `S:1099` sort, `A:2125` | local |
| Global sort by Duration and Random | Done | Global sort menu; Duration on whole seconds with missing counted as 0, as Eagle | local |
| A folder or smart folder keeps its own sort, or follows Global Settings | Done | `OU:178` sort_entries, `OU:644` own_sort_menu, `O:280` finish_order | local |
| Manual sort by dragging items inside a folder | Done | `O:309` manual_sortable, `S:1122` sort_manual, OrgAction::MoveManual | local |
| Natural (numeric-aware) title order | Done | `S:1060` natural_cmp | local |
| Trash sorted by deletion time | Done | `L:169` | local |

### 5. Sidebar views and Quick Access

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| All, Uncategorized, Untagged, All Tags and Trash, with counts | Done | `A:1740` nav_rows, `L:88` nav_counts | local |
| Uncategorized also counts items whose only folder no longer exists | Done | `logic::scope::uncategorized`: no folders, or exactly one id that names no folder, as Eagle | local |
| Random view | Done | `L:19` Scope::Random | local |
| Random refresh button (key R) | Partial | R key and View ▸ Shuffle Again; no on-screen button | local |
| Recently Used: items the user recently opened | Done | Opened items, newest first; trashed items listed too, as Eagle | local |
| Filter sidebar items | Done | Folders and smart folders filtered, Quick Access not (as Eagle); headers with no match hide; ⌥⇧F focuses the field | local |
| Go to Folder: quick-search folders, smart folders and tags, with history | Done | `app/goto.rs`-style dialog with history; also an Items tab as Eagle 4 | local |
| Quick Access: add or remove a folder or smart folder | Done | `A:1842`, `OU:391`, `E:595` | local |
| Quick Access: reorder by dragging, drag a folder in to add it | Done | Entries keep their whole JSON object when moved | local |
| Quick Access: drop items on a row to add them to that folder | Done | Drop target rows | local |
| Expand/collapse a folder, all folders, or the same level | Done | Expand/collapse entries in the folder and smart folder menus; ⌘, ⌥ and ⌘⌥ clicks on carets | local |
| Double-click on a sidebar row renames or collapses, per preference | Done | `Prefs::sidebar_double_click` for folder and smart folder rows | local |

### 6. Folders

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| New folder (sidebar +, shortcut) | Done | `A:1590`, Command::NewFolder | local |
| New subfolder | Done | `A:1836` | local |
| New Folder Below (sibling) | Done | `Action::CreateSibling` | local |
| A new child or sibling inherits the parent's icon and colour | Done | `edit.rs` `create_folder` copies `icon` and `iconColor` from the parent | local |
| Rename (inline) | Done | `A:1839` | local |
| Batch rename selected folders | Done | `Library::rename_folders` with Eagle's templates (`crates/falcon-library/src/rename.rs`), preview dialog, asks first from 10 folders | local |
| Delete a folder and its subfolders | Done | `A:1855`, `E:507` | local |
| Delete-folder checkbox "Move items within folders to Trash too" (on by default, can be unticked) | Done | `Library::delete_folders(keep_items)`; the box is ticked by default; a delete covering 50 or more items asks again, as Eagle | local |
| Delete several selected folders at once | Done | `Library::delete_folders`, one undo step; Quick Access entries removed | local |
| Move a folder by dragging (above, below, into) | Done | `OU:255` folder_drop, `O:135` tree_drop | local |
| Confirmation dialog when dropping a folder | Done | Eagle asks nothing on a drag either; only its Move Folder dialog confirms, which falcon's does | local |
| Move Folder dialog with destination search | Done | `folders_ui` `move_dialog`; Eagle's doesn't create a folder from the search either | local |
| Move Up, Down, To Top, To Bottom | Done | Edit ▸ Move Folder ▸ Up / Down / To the Top / To the Bottom and ⌘] ⌘[ ⇧⌘] ⇧⌘[ (`Action::Arrange`); not in the folder context menu | local |
| Sort folders by title for this level or all levels, A-Z or Z-A, with confirmation | Done | `OU:327`, `O:352`, `OU:1025` confirm | local |
| Duplicate a folder | Done | `Library::duplicate_folder` | local |
| Folder icon from the named icon set | Done | `OU:309` icon_grid, `folder_icons.rs:171` | local |
| Folder icon as an emoji | Missing | Unknown icon strings fall back to the plain folder (`folder_icons.rs:171`), so emoji icons set in Eagle don't show | local |
| Folder colour (8 colours or none), also tinting the guide line | Done | `OU:321` color_row, `A:1881` | local |
| Change icon or colour of several selected folders | Done | `Library::set_folders_icon` from the multi-folder menu | local |
| Folder cover: set an item as cover | Done | `image_edit.rs:92`, `E:465` | local |
| Folder cover: falls back to the first item, then to subfolders | Done | `covers.rs:44` | local |
| Reset a folder cover | Missing | `E:465` accepts None, but no UI calls it | local |
| Folder password: create with hint, change, remove, Lock Now | Done | `OU:270`, `OU:1067`, `O:202` | local |
| Locked folder hides its items and subfolders until unlocked; unlock screen shows the hint | Done | `O:160` hidden_folders, `OU:676` unlock_screen | local |
| Unlock a folder with Touch ID | Missing | none | local |
| Folder description, edited in the inspector | Done | Saves 1 s after typing stops and on blur, one undo step (`Action::SetFolderDescription`) | local |
| Rename a folder from the inspector name field | Done | `scope_inspector`: Return or blur saves through `Action::RenameFolder`; an emptied name reverts | local |
| Folder auto-tagging: tags added to every item put in the folder, including the parent folders' tags | Done | `crates/falcon-library/src/autotag.rs` (own tags then each parent's, deduped, in Eagle's order); applied on import, paste, Add to Folder, the inspector picker and drops. `extendTags` is derived, as Eagle does, not stored | local |
| Folder settings dialog (name plus auto-tags), Cmd+Shift+R | Done | `organize_ui.rs` auto_tag_dialog; `shortcuts.rs` `edit.folder.setting`; saving appends the tags to the folder's items, as Eagle does | local |
| Show Subfolder Content (folder view includes descendants' items) | Done | `logic::folder_members`, ⌥⌘7 | local |
| Subfolder strip with covers at the top of a folder | Done | `covers.rs:65` subfolder_strip | local |
| Folder item counts | Done | `L:67` folder_counts | local |
| Folder inspector: item count, size, date imported, password state | Done | `A:2926` | local |
| Folder inspector: set, change or remove password buttons | Partial | Shows only "Set" or "None"; the actions are in the context menu | local |
| Copy Folder Link (eagle:// link) | Done | Folder and smart folder menus | local |
| Export folder (computer, .eaglepack) | Done | Folder menu Export ▸ … and the inspector's Export button (`export_ui.rs` `tree_export_menu`, `scope_export_button`) | local |
| Clear Invalid Content (empty or broken folders) | Missing | Cross-ref: library agent | local |

### 7. Smart folders

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Create a smart folder with a name and rules | Done | `A:3222` smart_dialog_ui, `SM:297` | local |
| Edit rules, rename, delete (with confirmation text) | Done | `OU:379`-`421`, `SM:354`/`366`/`373` | local |
| Nested smart folders: a child narrows its parent's rules | Done | `SM:77` contains | local |
| Smart folder groups (no rules, show the union of the children) | Done | `SM:58` is_group, `OU:492` | local |
| Drag to reorder and nest | Done | `SM:384`, OrgAction::MoveSmart | local |
| Icon and colour | Done | `OU:408`, `SM:417` | local |
| Smart folder's own sort | Done | `OU:400`, `SM:429` | local |
| Add to Quick Access | Done | `OU:394` | local |
| Duplicate a smart folder | Done | `smart::duplicate_smart_folder` | local |
| Description | Done | Edited in the inspector when a smart folder is open and nothing is selected (Eagle edits it there too, not in the rules dialog) | local |
| Several conditions, each with its own Any/All and true/false (NOT), AND-ed together | Done | `smart_draft.rs` drafts/conditions and `organize_ui::smart_conditions_ui`: every condition is edited, each with must/must-not and all/any | local |
| Editing a smart folder keeps its other conditions and rules | Done | Untouched conditions and rules are written back verbatim, edited ones keep their unknown keys, and rules the dialog can't show stay as read-only Unsupported rows until deleted (`smart_draft.rs`, test `eagle_smart_folder_survives_an_edit`) | local |
| Live "N items found" count while editing rules | Missing | none | local |
| Batch rename, recolour or delete several smart folders | Done | Multi-smart-folder menu in `folders_ui`, looping the `smart.rs` calls in one undo step | local |
| Export a smart folder | Done | Smart folder menu Export ▸ directory or `.eaglepack` (`export::plan_smart`, `PackSource::Smart`) | local |
| Rule: name, URL, note, with equals, contains, excludes, begins with, ends with, empty, not empty | Done | `smart.rs` text rules; every method is in the dialog for each text property. Begins/ends-with are anchored regexes as in Eagle | local |
| Rule: regex method | Partial | `smart.rs` uses the `regex` crate, lower-casing as Eagle; no look-around or backreferences (documented difference) | local |
| Rule: folder name | Done | `SM:111`, `A:243` | local |
| Rule: folders (in any, all, equal, excludes, empty, not empty) | Done | `smart.rs`; the dialog has a folder checklist (`organize_ui::rule_fields_ui`) | local |
| Rule: tags (any, all, equal, excludes, empty, not empty) | Done | `SM:124`, `A:244` (typed list, no tag picker) | local |
| Rule: type (is, is not; extensions and groups) | Done | `SM:126` | local |
| Rule: rating (is, is not, contains) | Done | `smart.rs` rating, including "contains" over the stars as text; all three in the dialog | local |
| Rule: shape including custom W:H | Done | `smart.rs` shape; the dialog has custom W:H fields | local |
| Rule: colour (similar, almost the same, grayscale) | Done | `SM:157`, `SM:168` | local |
| Rule: width | Done | `SM:113`, `A:249` | local |
| Rule: height | Done | `smart.rs`; in the dialog | local |
| Rule: file size with KB or MB unit | Done | `smart.rs`; the dialog offers KB/MB and =, <, >, between | local |
| Rule: BPM | Done | `smart.rs`; in the dialog | local |
| Rule: Date Added (on, before, after, between, within last N days) | Done | `smart.rs` compares local calendar days for "on"; the dialog takes dates typed as YYYY-MM-DD and a "within N days" count. No date picker | local |
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
| All Tags page: every tag with its count, filter box, A-Z sections | Done | `A:2745` tag_page | local |
| Click a tag to show its items | Done | `A:2840` Scope::Tag | local |
| Rename a tag everywhere (items, history, starred, groups) | Done | `A:2853`, `E:131` | local |
| Merge tags: select several and merge them into one name | Done | `Library::merge_tags` through the merge dialog, 2–5 tags; rename to an existing name still merges two | local |
| Delete a tag (with confirmation) | Done | `A:2860`, `E:169` | local |
| Star and unstar tags; Starred section | Done | `A:2856`, `E:187`, `A:2751` | local |
| Select several tags (click, Shift, Cmd, rubber band) and act on all | Partial | Click, ⌘ and ⇧ picks with the tag menu acting on all (`tags.rs`); no rubber band | local |
| Tag groups: create, rename, colour, description, reorder, delete | Done | `OU:722`-`900`, `E:214`-`330` | local |
| Move a tag to a group, remove from group, drag a tag onto a group | Done | `OU:919` tag_group_menu, DragTag | local |
| Add a tag to more than one group | Done | Add to Group keeps the tag's other groups (`add_tags_to_group_too`) | local |
| Find items with any tag in a group | Done | Group heading menu | local |
| Tag sidebar: All, Starred, Uncategorized (tags in no group), Groups | Partial | Toggle between A-Z and group view (`OU:724`); falcon lists "Ungrouped" tags in group view, but there are no separate Starred or Uncategorized pages | local |
| Sort tags by name or count, group by alphabet or groups | Done | All Tags page sort menu | local |
| Tag group colour dot on chips | Done | `A:2827` tag_dot | local |
| Recent tag history (last 120 used) kept in tags.json | Done | `tagging.rs` `add_tags_to_items` and `push_history`: each added tag moves to the front, 120 kept, a tag removed from its last item leaves the history | local |
| Clear tag history | Done | Button on the All Tags page, `Library::clear_tag_history` | local |

### 9. Inspector (one item)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Preview; double-click to open | Done | `A:3034` | local |
| Colour palette swatches | Done | `A:3053` | local |
| Click a swatch to find items of similar colour | Done | `A:3066` | local |
| Copy the colour as HEX, RGB or RGBA | Done | Swatch right-click: HEX, RGB, RGBA, HSL, HSV, HWB, CMYK (`colors.rs`) | local |
| Set a palette colour as the primary colour (`marked`) | Partial | Stored as Eagle writes it (`marked` true/false); colour search still ignores it | local |
| Re-analyse colours | Done | `image_edit.rs:107` Refresh Palette | local |
| Name (title) editing | Done | `A:3082` | local |
| Click the extension to change it | Done | Inspector Type row opens a checking dialog; only the original is renamed, as Eagle | local |
| Note editing | Done | `A:3092` | local |
| URL editing, validated as Eagle does | Done | `urls.rs` `url_accepted`: web URLs and existing paths, as Eagle; Open Link icon beside the field | local |
| Open Link in Browser button | Done | Open Link button beside the URL field, View ▸ Open Link in Browser, ⇧⌘O | local |
| Tags: chips, remove, type a new tag | Done | `tag_field`: chips with × and a + that opens the picker, where typing happens, as Eagle | local |
| Tag picker with Selected, Starred, Recent, Recommended and groups, search, column sizes | Done | `app/tag_picker.rs`, `tags.rs`: search and ranking, Starred, Recent (latest 12 in use), Recommended, groups with a Tab sidebar, settings; no Pinyin or jieba search, no dragging or resizing | local |
| Copy, paste and clear an item's tags | Done | `Library::paste_tags`, `clear_tags`; one undo step and a notice | local |
| Folders: chips, remove, Add Category | Done | `folder_chip` in app.rs; removal is the × on the chip | local |
| Click a folder chip to open that folder | Done | `folder_chip` in app.rs: the click opens the folder with the item still selected and scrolled to; the tooltip shows the folder path | local |
| Folder picker with search, Recent, Selected and create-new | Done | `add_to_folders.rs`; modal applied on Save (Eagle: popover applied on close) | local |
| Comments list: jump to, edit, delete | Done | `comments.rs:427` | local |
| Rating stars; click the same star again to clear | Done | `A:3332` | local |
| Dimensions, size, type, date imported, created, modified | Done | `A:3196`-`3203` | local |
| Duration and BPM | Done | `media_view.rs:284` | local |
| EXIF: camera, ISO, aperture, focal length, shutter, timestamp | Done | `media_view.rs:292` | local |
| Collapse or expand inspector sections | Done | `fold_heading` on Properties, Tags, Folders in every inspector and Comments in the viewer | local |

### 10. Batch editing (several items selected)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Preview stack and "N items selected" | Done | `A:2943` | local |
| Add a tag to all; remove a tag they share | Done | The picker and the chip × for several items | local |
| Add all to a folder | Done | `A:2988` | local |
| Show the folders they share, remove all from one | Done | `shared_folders` chips with × in the multi-item inspector | local |
| Edit the name, note or URL of all at once ("Multiple titles", "multiple URLs") | Done | `set_notes`, `set_urls` from the multi-item inspector; Eagle has no batch name either | local |
| Rate all | Done | `A:3003`, Action::BatchRating | local |
| Total size | Done | `A:3007` | local |
| Batch rename with a template (%NN, $D, original name, tags, folders, index, date) and find/replace with regex | Done | Rename N Items dialog (`rename.rs`): `%NN`, dates, original name, tags, folders, `%F`, index, find/replace | local |
| Confirm bulk actions on many items | Done | `push_bulk` / `push_bulk_at`: rating, trash, drops, batch notes/URL, the folder panel, Add to Last Used, Pin/Unpin, Show in Finder and Open in default app on several, Remove from Group; folder delete at 50 items. Eagle also gates fonts and plugins (out of scope) | local |
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
| Find identical files | Done | `DV:76`, `duplicates.rs:91` content hash | local |
| Find similar-looking images, with a similarity slider | Done | `duplicates.rs:173` perceptual hash, `DV` slider | local |
| Choose the method (same or similar) before scanning | Done | Duplicates page: method, range, similarity | local |
| Scan all, the current list, or the selected items | Done | `merge.rs` scan ranges | local |
| Progress while scanning | Done | `DV` progress bar | local |
| Best copy picked automatically for similar groups (highest quality) | Done | Most pixels, then bigger file of the same format, else Eagle's format order; the kept copy can be changed | local |
| Remove an item or a group from the results | Done | Duplicates page | local |
| Merge: keep one file and combine the others' folders, tags, link and notes into it, choosing the file name | Done | `crates/falcon-library/src/merge.rs`: the kept item takes the folders, tags, notes, link and highest rating; the others are trashed, nothing deleted; one undo step; always asks first | local |
| Summary of files deleted and space saved | Done | After the merge | local |
| Move duplicates to Trash | Done | `DV:177` | local |

### 12. Trash and ratings

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Move to Trash (menu, key, drag onto Trash) | Done | `A:2537`, `A:1765` | local |
| When an item is in several folders, ask whether to remove it from this folder or trash it | Done | Trashing inside a folder asks "only this folder" or "trash all"; the answer is kept for the session | local |
| Restore selected items | Done | `A:2531`, `E:94` | local |
| Restore All Items (with confirmation) | Done | Trash menu | local |
| Delete permanently (with confirmation) | Done | `A:2534`, `E:108` | local |
| Empty Trash (with confirmation) | Done | `A:1760`, `E:122` | local |
| Ratings stored as 1-5, key removed when cleared | Done | `A:3188`, `A:1215` | local |
| Toast "rated as N star" | Done | "Rated N stars" / "Removed the rating" notice from the rating keys and menu | local |

### 13. Comments (annotations)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Region comments on images: draw, move, resize, edit, delete | Done | `comments.rs` | local |
| Timed comments on videos | Done | `video_view.rs:472` | local |
| Filter by comments | Done | Comment filter chip (`FilterKind::Comment`), Find menu, ⌥-key | local |
| Search comment text | Done | `search_text_in` includes comment texts when the Comments scope is on (default on) | local |
| Smart folder rule on comments | Done | `smart.rs` `comments_rule`, offered in the dialog | local |

### 14. Saved filters

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Save the current filter (and search) under a name | Done | `OU:958`, `E:334` | local |
| Apply, rename, remove, reorder saved filters | Done | `OU:952`-`985` | local |
| Saving keeps every active rule | Done | `SavedFilter::rule_from_filter` in model.rs writes dates (Eagle's shared `import.range`), duration (`min`/`max`/`unit`), camera and font in Eagle's key names; Date Created goes under falcon's own `btime` key | local |
| Applying restores every saved rule | Done | `SavedFilter::to_filter` restores every rule; only Eagle's AI rules (`image`, `semantic`) are skipped with a warning | local |
| Saved filters button appears in the filter bar | Done | `OU:954` | local |

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
4. **Saved filters drop and skip rules.** Saving drops date, duration, camera and font. Applying skips them, plus comments, ratio shapes and excluding Uncategorized.
5. ~~Folder description and inspector rename don't save.~~ Fixed.
6. ~~Clicking a folder chip in the inspector removes the item from the folder.~~ Fixed: it opens the folder; the chip's × removes.
7. **No size or dimensions filter chips.** The engine supports them, but there's no UI.
8. **No Comment filter, and comments, camera, text content and fonts aren't searched.** There's also no Search Scope menu.
9. **The smart folder dialog** has no date picker and shows no live hit count.
10. ~~No tag picker.~~ Fixed.
11. **No exclude, search or tag-group options in the Tag, Folder and Type filters.** Also no type groups (video, audio, font...).
12. ~~Remove from current folder is missing, and the "remove from folder or trash" prompt.~~ Fixed: ⇧⌘⌫, and trashing in a folder asks "only this folder" or "trash all" when items are filed elsewhere too.
13. ~~No Show Subfolder Content toggle.~~ Fixed.
14. **Batch rename of items is missing**, and so is batch name/note/URL editing for several items (folders can be batch-renamed).
15. **Duplicates can't merge.** Folders, tags, notes and links aren't combined; there's no best-copy pick, no scope (current list or selected), and no remove-from-results.
16. ~~No Go to Folder quick search.~~ Fixed.
17. ~~No multi-select of folders.~~ Fixed, for folders and smart folders.
18. ~~Missing folder commands.~~ Fixed, including Add to Last Used Folder.
19. Eagle 4 stores no emoji icons (its "emoji" submenu is the named-icon grid) and has no reset-cover command; a new folder inherits the parent's icon and colour.
20. ~~No Pin to Top for items, and no Open File Location.~~ Fixed.
21. **Tag manager gaps**: no Uncategorized or Starred tag pages, no rubber-band tag selection, and rename or merge don't reach folder auto-tags or smart-folder rules.
22. ~~Quick Access can't be reordered, dragged into, or used as a drop target.~~ Fixed.
23. **Filter bar gaps**: no Filter Lock toggle (falcon always keeps the filter and search when switching views), no add or pin filters, no ratio shapes (4:3, 16:9, custom), no counts for type, shape and rating.
24. ~~Global sort lacks Duration and Random.~~ Fixed.
25. **Rating gaps**: no rating shortcuts (0-5) and no rating toast.
26. ~~Palette gaps.~~ Fixed; colour search still ignores the primary colour.
27. ~~No change-extension in the inspector; URL isn't validated; no Open Link button.~~ Fixed.
28. ~~Recently Used isn't a list of opened items; Uncategorized ignores stale folder ids.~~ Fixed (R reshuffles Random).
29. **Folder deletion**: the trash checkbox can now be unticked; there's no move confirmation, and Touch ID unlock is missing.
30. **Search extras**: no Search All, no Chinese traditional/simplified matching, no pinyin sidebar search. Falcon's own fuzzy matching is a deviation from Eagle.
31. **Minor UI gaps**: Smart Folders and Folders section headers don't fold; the bulk-action gate doesn't cover every newer command yet.

Online/plugin only (not ranked): Visual Match, Semantic search, the search-by-image button.

## Import, export, library, preferences and integrations


Scope: what users can do in this area. Eagle paths are relative to `eagle-src/app/`. "B:" means `app/app.bundle.js`, "BG:" means `app/js/background.js`, "PH:" means `app/preferences.html` (the `ng-model` list), "DP:" means `app/js/default-preferences.js`, and "en:" means a key in `i18n/en.js`. Falcon paths are relative to the repo root. Every falcon claim below was checked in code, not in the docs or ROADMAP.

Status meanings: **Done** means it works in the falcon app, or the API for API rows. **Partial** means it exists only in the CLI or library crate, or behaves differently. **Missing** means it does not exist.

Two cross-cutting facts explain many rows:
- Falcon's native menu bar (`menu.rs`) carries File, Edit, Find, Organize, View, Window and Help; Eagle's online and plugin entries are left out.
- Falcon's `Prefs` struct (`crates/falcon-app/src/prefs.rs:21-35`) holds six fields: hover zoom, IPTC keywords, API on/off, API port, collect folder and theme. The Preferences window has four sections: General, Preview, Shortcuts and Developer (`prefs.rs:138-147`). Every other Eagle setting has no switch in falcon. Where falcon hard-codes the behaviour, the row says so.

---

### 1. Import sources

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Drag files from Finder onto the main window to import them into the current folder | Done | `crates/falcon-app/src/app.rs:3600-3605` (dropped files go to `import`); the folder comes from `scope_folders` `app.rs:857` | local |
| Drag a folder onto the sidebar to import it with its subfolder tree recreated as Eagle folders | Done | `crates/falcon-library/src/folder_import.rs` (walk, plan, `create_folders`); `import_ui.rs` `import_folder_trees` | local |
| Import Folders from a file dialog (several folders, structure kept) | Done | `import_ui.rs` `run_import_request`; Import Folders… and Import Files… in the library switcher and on the empty All and folder pages | local |
| Warning before importing a very large folder ("contains N items, continue?") | Done | `folder_import::verdict`: 2,000 files or more ask first, as Eagle | local |
| Refuse folders deeper than 15 levels or with 100k+ files | Done | `folder_import::verdict`, Eagle's numbers | local |
| Dropping or picking a `.library` folder opens it as a library instead of importing it | Done | `logic::split_libraries` separates `*.library` paths before the import walk in `import_into`; the first opens through the sandbox-guarded `App::open`, the rest are named in a toast. Like Eagle, a `.library` nested inside a dropped folder is not special-cased | local |
| A Pixave `.pxvlibrary` folder is skipped by folder import | Done | `folder_import.rs` walk (everywhere, not only the dialog) | local |
| Paste files copied in Finder (⌘V) | Done | `app.rs:961-967` (`arboard` file list) | local |
| Paste a copied bitmap image as a new PNG item | Done | `app.rs:968-977` (named "Pasted image <date>") | local |
| Paste a URL from the clipboard: an image URL is downloaded, a web page is saved as a bookmark | Missing | `paste_into` handles only files and images | online |
| Import from links: a dialog to paste many URLs, one per line, prefilled from the clipboard | Missing | — | online |
| Web bookmark items (save a page URL as an item, with a capture of the page) | Missing | `v2.rs:301-302` rejects `bookmarkURL` with "bookmarks are not supported" | online |
| Drag an image or link from a web browser into the window | Missing | Only `dropped_files` with local paths are read (`app.rs:3600`) | online |
| Batch-download panel: pick images from a page, filter by type and size, select all or invert | Missing | — | online |
| Screen capture of an area, a window or the full screen into the library | Done | `capture.rs`: area (`screencapture -i`), window (`-w`), full screen (`-D` of the pointer's display); from the menu bar icon, the global keys and `Command::Capture`; Eagle's naming "Screenshot - date", format conversion, @2x halving | local |
| Watched auto-import folder: files dropped into it are imported automatically | Partial | `auto_import.rs` polls `<chosen>/<library name>` every 2 s and imports settled files into the root; imported files go to the Trash as Eagle deletes them (preference `auto_import_delete`, on by default, Trash instead of a hard delete). Still Partial: polling instead of file events, only the shown library is watched, any format is accepted | local |
| Auto-import rejects network drives and missing paths | Done | `auto_import::is_network_volume` (`NSURLVolumeIsLocalKey`), `prefs::auto_import_refusal`; a vanished folder pauses with a notice | local |
| Auto-import skips `.tmp` files, tiny files and unsupported types, and waits until a copy has finished | Done | `auto_import::skip` and `Settle` (size and mtime unchanged for 3 s); any extension is accepted, as a drop is | local |
| Import an Eagle Package (`.eaglepack`) from the File menu or by opening the file | Done | File ▸ Import submenu, the switcher, a drop, and Finder Open With / Dock drop through `open_files.rs` and the bundle's document types | local |
| Confirm the disk space an `.eaglepack` will use before importing it | Done | `preview_pack` confirmation | local |
| When an `.eaglepack` holds items already in the library, ask "N repeated files, import anyway?" | Done | Count in the title, Import / Leave Them Out, Esc imports everything, as Eagle | local |
| Merge another Eagle library into the current one (folders and items), refusing the library already open | Done | `crates/falcon-library/src/library_merge.rs`: Eagle's id matching for folders, smart folders and tag groups; item folders copied in the background with progress and Stop; the open library is refused | local |
| Import an Inboard backup folder (`InboardBackup.json`), with its folders recreated | Done | `ImportOptions::trashed` and `added_at`; folders recreated | local |
| Import a Pixave library | Missing | — | local |
| Import from Billfish | — (not counted) | — | — |
| Import from Pinterest | Missing | — | online |
| Import from Huaban | Missing | — | online |
| Import an ArtStation artist's portfolio by URL into a folder, all at once or picked by hand | Missing | — | online |
| Add files by opening them with the app from Finder (Eagle registers for about 36 image, video and design types, folders and packages) | Partial | `CFBundleDocumentTypes` in `scripts/bundle.sh` (Eagle's 30 types, `.eaglepack`, `.library`, folders) and the open-documents Apple event (`open_files.rs`) importing or opening them; not yet checked in a built bundle | local |
| Add items from another app through the HTTP API (`addFromPath`, `addFromPaths`, `addFromURL`, `addFromURLs`, v2 `item/add`) | Done | `crates/falcon-server/src/lib.rs:369-372,386`; data URLs and http(s) downloads in `add_url` (`lib.rs:753-779`) | local, or online for URLs |
| Add an item from base64 data (v2 `item/add` with `base64`) | Done | `crates/falcon-server/src/v2.rs:303-305` | local |
| Import into a folder chosen in the collect window | Done | `app.rs:768-776` | local |

### 2. Import options and behaviour

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Copy the original into `images/<ID>.info`, keep the source mtime, make a thumbnail and palette | Done | `crates/falcon-library/src/import.rs:79-147,173-207` | local |
| Imports run in the background with a progress display | Done | `app.rs:888-938` (background thread and progress bar) | local |
| Upload queue panel listing failed imports with reasons (download failed, timeout, cannot decode and so on), plus Retry All, Copy to Clipboard and Clear All | Done | `import::Failure` / `FailKind` per file; `import_ui.rs` `error_panel` with Retry All, Copy to Clipboard (the files), Clear All; opened from a toast. Local-file reasons only | local |
| "Import Successful, N image(s) imported" notification | Partial | In-window note plus a system notification when bundled | local |
| Duplicate check on import (Eagle's metadata key), with a dialog offering Import, Don't Import, Keep both, Use existing file and Apply to all | Done | `import_check.rs` (same size and extension, then a BLAKE3 hash of the whole file), `crates/falcon-app/src/import_dupes.rs`, `app/dupes_ui.rs`: Eagle's two choices (use existing, keep both), Import, close = don't import, apply to all. One thumbnail shown, not two | local |
| Setting: notify me when importing duplicates | Done | `Prefs::notify_duplicates`, on by default; off imports duplicates silently as Eagle does | local |
| Folder auto-tagging: tags set on a folder are added to every item imported or pasted into it | Done | `ImportOptions::folder_auto_tags`, set by `Prefs::import_options`; applied in `commit_import` | local |
| Screenshot auto-tag ("Screenshot" tag) | Done | "Screenshot" tag when the item has no tags, as Eagle (`screenshot.autoTagging`) | local |
| Add IPTC and XMP keywords as tags (setting) | Done | `prefs.rs:24,327`; `import.rs:121-123` | local |
| Font style tag (Regular, Bold, Italic) added on import (setting) | Done | `Prefs::font_style_tags` in the Preview pane, on by default | local |
| Auto-select the new items after import (setting) | Done | `Prefs::auto_select`, on by default; selects the new items shown in the view and scrolls to a single one | local |
| Unsupported formats are refused with an "unsupported format" dialog | Done | `import::is_recognized`: like Eagle, only extensionless files and unfinished browser downloads (`.crdownload`, `.part`, `.download`) are refused, into the error list; every other type imports | local |
| Empty files are refused (except `.txt`) | Done | `import.rs:88-90` | local |
| Hidden files are skipped during folder walks | Done | `app.rs:149-151,872-876` | local |
| Not-enough-disk-space check before copying | Done | Eagle has no pre-copy check on import either; ENOSPC is reported per file as "Not enough disk space" in the error list | local |
| Filename sanitising and length limit | Done | `import.rs:411-430` | local |
| Cut mode: files staged in app data (pasted or downloaded) are moved, not copied | Done | Pasted temp files are deleted after import (`app.rs:893-897`) | local |
| Choose where a screenshot or extension item is saved (collect prompt) | Partial | The collect window's waiting-screenshot mode for area and window captures (`showCaptureCollectModal`); the extension doesn't exist | local |

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
| Overwrite prompts when exported files or folders already exist | Done | Replace / Keep Both / Cancel (`export_ui.rs`); Eagle offers Overwrite or Cancel | local |
| Export selected items as `.eaglepack` (⇧⌘E) | Done | `export_ui.rs`; `pack.rs` `plan_pack` / `write_pack` run off the UI thread | local |
| Export a folder (with subfolders) as `.eaglepack` | Done | Folder menu and inspector button | local |
| Export a smart folder as `.eaglepack` | Done | `pack.rs` `PackSource::Smart`: one flat folder, as Eagle 4; CLI `export --smart` | local |
| Export in a chosen format and size (custom export) | Done | ⇧E, Export ▸ Export in Format…: JPG, PNG, WebP, quality, longest side (`export::plan_converted`). Eagle's dialog is a plugin outside its source, so the options are falcon's own | local |
| Export metadata to CSV (ID, name, extension, size, URL, notes, tags, folders, comments and so on) | Done | `export::csv`: Eagle's 16 columns in order, BOM and `\n` line ends, falcon's own headers; item menu and inspector | local |
| "Export Successful, N item(s) exported" notification | Partial | In-window note, Finder reveal and a system notification when bundled; CSV export posts none | local |
| Drag items out to Finder or other apps | Done | `app.rs:489,1451,3714` (`native_drag`, `drag_image`, `drag::start_drag`) | local |
| Copy the selected files to the clipboard (⌘C) | Done | `app.rs:941-953` | local |
| Copy File Path (⌥⌘C) | Done | All selected items, one per line (`export_ui.rs`) | local |
| Copy folder path (⌥⇧⌘C) | Done | The selected items' `<id>.info` folders, as Eagle | local |
| Copy Name | Done | `export_ui.rs` `item_copy_menu` | local |
| Copy Thumbnail (as an image) | Done | One item: image on the clipboard; several: the thumbnail files, as Eagle | local |
| Copy Base64 | Done | One item: data URL of the original for jpg/png/webp/gif, else of the thumbnail | local |
| Copy Eagle Link (`eagle://item/<id>`) | Done | Copy ▸ Eagle Link in the item menu, Edit ▸ Copy Details, `edit.copy.eaglelink` (unbound, as Eagle) | local |
| Copy Link (the item's source URL) | Done | Copy ▸ Link copies Eagle's local API link per item (`edit.copy.link`); the source URL keeps its own entry (`edit.copy.sourceurl`) | local |
| Copy the image to the clipboard as pixels plus file reference (the AppleScript helper) | Partial | Falcon copies a file list only (`app.rs:948-949`) | local |
| macOS Share sheet for items | Done | Share… in the item menu over the selected files (`share.rs`, `NSSharingServicePicker`) | local |
| Add selected items to another library (from the recent list, or choose a path) | Missing | — | local |
| Convert items to WebP to save space | Missing | — | local |
| Save the current video frame as an item or copy it | Done | Video right-click menu and ⇧S / ⇧C; also in the preview window | local |

### 5. Library management

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Create a library | Done | `app.rs:779-788`; `crates/falcon-library/src/library.rs:98-122` | local |
| Create-library dialog with a name field, name validation and folder choice | Done | Eagle's forbidden characters, folder picker | local |
| Warn when creating a library in Desktop, Documents or Downloads | Done | Desktop and Downloads, as Eagle (substring match) | local |
| Refuse a path that already holds a library | Done | `library.rs:101-106` refuses any existing path | local |
| Refuse libraries inside the app's install folder | Missing | — | local |
| Open an existing library | Done | `app.rs:790-794,594-618` | local |
| Reject a folder that is not a library | Done | `library.rs:53-55` (`NotALibrary`), shown in the empty state (`app.rs:3537-3539`) | local |
| Switch library (⌘L) through a switcher listing libraries | Done | `app.rs:998-1001,1607-1650` | local |
| Recent libraries history | Done | `crates/falcon-app/src/workspaces.rs:119-129`, `app.rs:1627-1635` (10 entries, `app.rs:1436`) | local |
| Clear library history | Done | Switcher menu | local |
| Remove one library from the list | Done | Switcher menu; missing libraries are kept and shown dimmed | local |
| Delete a library folder from the list menu | Missing | Not an Eagle feature: its "Delete folder" string is the Remove Icon confirmation; not built, on data-safety grounds | local |
| Set or remove a library icon (`icon.png`) | Done | `library_icon.rs`: shorter side scaled to 64 px, written atomically; Change Icon / Remove Icon in the switcher | local |
| Warn before switching while tasks run (Force Switch), or block while an AI action runs | Missing | Imports keep running against their own shared library handle; there is no prompt | local |
| Several libraries open at once | Done (beyond Eagle) | `workspaces.rs` (several workspaces), `app.rs:621-689` | local |
| Show the library in Finder | Done | `app.rs:1643-1645` | local |
| Missing-library screen with Try Again, Select another path and Create a new library | Done | At launch, from a missing recent entry, and when an open library vanishes | local |
| Relocate a library that moved | Done | Select Another Path; the old path leaves the history | local |
| Rename a library | — (not counted) | — | — |
| Refuse a library made by a newer Eagle and prompt to update | Done | Version check against 4.0.0 with Eagle's ordering; an alert, since falcon has no updater | local |
| Automatic `metadata.json` backups (on change and every 30 minutes, up to 100 kept) | Done | `crates/falcon-app/src/backups.rs` schedules a backup 1 s after a metadata write and every 30 minutes, run off the UI thread by `run_backups` in app.rs; `flush_and_backup` takes a waiting one when a library closes or the app saves state. Written by `backup::backup_library` | local |
| Restore a damaged `metadata.json` from the newest good backup on open | Done | `App::open` and `follow_api_switch` use `Library::open_or_restore`; `restored_note` names the backup used in the toast. The HTTP API's own `library/switch` still uses `Library::open` | local |
| Keep a per-library backup copy in app data (`eagle-backup/<hash>.json`) | Done | `~/Library/Application Support/Falcon/eagle-backup/<hash>.json` with Eagle's FNV-1a path hash, written with every new backup | local |
| Empty cache and reload the library (forced rebuild) | Done | Rebuilds `mtime.json` from the items as Eagle does | local |
| Repair tool "Clear Invalid Content" (removes empty or broken item folders, then reloads) | Partial | Unreadable items are skipped and reported on open (`library.rs:63-73`, `app.rs:607-610`), but cannot be cleaned up | local |
| Refresh thumbnails for the selection | Done | `crates/falcon-app/src/image_edit.rs:104-106,151-171` | local |
| Refresh the palette for the selection | Done | `image_edit.rs:107-109` | local |
| Detect external changes to `metadata.json` and `mtime.json` (sync tools) and reload | Done | `sync.rs`, `watch.rs`: polled every 4 s, parsed off the UI thread, nothing written back; a vanished folder closes the library | local |
| Cloud-sync conflict detection (disk copy newer than memory, so warn and reload) | Done | Every metadata write first reads the disk copy and refuses if it is newer (`Error::SyncConflict`); the app alerts and reloads | local |
| iCloud placeholder handling (`.icloud` names, files not yet downloaded) | Done | `crates/falcon-app/src/icloud.rs` (dataless detection, background download, wait screen in `app/open_ui.rs:128-180`) | local |
| "File not found, sync may be in progress" hint for missing originals | Partial | iCloud files are handled; other missing originals get no specific hint | local |
| Library storage health warnings (read-only path, slow disk, NTFS on macOS, permission errors) | Done | Read-only file, no write permission, slow disk (falcon's own 1.5 s round-trip threshold), NTFS volume | local |
| Library loading progress screen | Done | `Library::open_with_progress` | local |
| Library stats (item count, size, trash count) | Partial | Sidebar counts (`app.rs:1744-1745`) and folder inspector items and size (`app.rs:2925-2928`); CLI `info` prints items, trash, folders and tags (`src/main.rs:112-121`). No library-wide stats in the app | local |
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
| General: language | Partial | Preferences ▸ General ▸ Language (`Prefs::language`, `FALCON_LANG` override), Cancel reverts; conversion of the viewers, sidebar and menus pending | local |
| General: UI zoom level (50 to 150%) | Done | `Prefs::ui_zoom` in Eagle's steps, previewed live; egui's own keyboard zoom is disabled so ⌘+/⌘− only zoom the grid | local |
| General: transparency (vibrancy) effects | Missing | — | local |
| General: show counts in the sidebar | Done | `Prefs::sidebar_counts` | local |
| General: menu bar icon | Missing | No status item | local |
| General: keep the Dock icon after the window closes | Done | Closing hides falcon (`dock.rs`); always on, no preference | local |
| General: launch at login | Missing | — | local |
| General: GPU acceleration | Missing | — | local |
| General: auto-select items after import | Done | `Prefs::auto_select` | local |
| General: notify on duplicate import | Done | `Prefs::notify_duplicates` | local |
| General: ask where to save items from the extension | Missing | — | local |
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
| Screenshot: enable the global shortcut, plus area, window and full-screen shortcuts | Done | Enable switch plus the area / window / full-screen rows, recordable in the pane (`Recorder::rows`) | local |
| Screenshot: ask where to save | Done | Collect-window prompt for area and window shots | local |
| Screenshot: notify when taken | Done | System notification when bundled (`notify.rs`) | local |
| Screenshot: auto-tag "Screenshot" | Done | "Screenshot" tag | local |
| Screenshot: also copy to the clipboard | Done | The finished picture is copied with arboard (not `-c`) | local |
| Screenshot: @2x on Retina | Done | Halves at scale ≥ 1.5 (Eagle checks exactly 2) | local |
| **Shortcuts:** rebind any command, search the list, Restore Defaults | Done | 147 rebindable rows with Eagle's ids, search field, Restore Defaults, recorder | local |
| Shortcuts: plugin shortcuts | Missing | No plugins | local |
| Global shortcuts (show Eagle, quick search from anywhere) | Done | `hotkeys.rs`, Global group in the shortcut table | local |
| **Notification:** sound effects master switch, plus per event (delete item, delete folder, screenshot, extension) | Missing | — | local |
| Notification: pop-up master switch, plus per event (screenshot, extension, duplicates, auto-import) | Partial | Master switch, duplicates, import done, auto-import, screenshot and export switches; the extension switch has nothing to control | local |
| Notification Center panel (history) | Missing | — | local |
| **Password:** enable the app password lock, change the password, Lock Now, Touch ID | Done | Set, change, remove, hint, Lock Now ⌥⌘L, Unlock with Touch ID (shown when available, fingerprint checked when turned on, as Eagle) | local |
| **Auto-Import:** enable, choose folder, open folder | Done | `prefs.rs` `auto_import` pane: enable, Choose Folder…, Open Folder, Make It for the watched subfolder | local |
| **AI Search** pane (install, enable or disable the local semantic-search plugin) | Missing | — | online/plugin |
| **AI Models** pane (shared API keys for OpenAI, Anthropic, Gemini and so on) | Missing | — | online/plugin |
| **Eagle MCP** pane (install or open the MCP server plugin) | Missing | — | online/plugin |
| **Developer:** show, copy and regenerate the API token | Done | Developer pane: empty by default like Eagle, Generate when empty, Copy / Regenerate / link when set; an empty token means Host-check-only auth (`authorized` in `falcon-server`) | local |
| Developer (falcon only): API on or off, and first port | Done (beyond Eagle) | `prefs.rs:339-358` | local |
| Developer: plugin developer tools | Missing | — | local |
| **Proxy** (server address and port, used for downloads and web search) | Missing | — | online |
| Color space (sRGB, Display P3, unmanaged) | Missing | — | local |

### 7. HTTP API

Falcon serves the API on 127.0.0.1 from port 41596 upward (never 41595), while a library is open in the app or through `falcon serve` (`lib.rs:69`, `prefs.rs:12-15`, `src/main.rs:85-92`). Routes are registered in `crates/falcon-server/src/lib.rs:355-411`.

#### 7.1 v1

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| `GET /` and `/api/application/info` | Done | version, build, platform, `showCollectModal` and a `preferences` subset (`lib.rs`) | local |
| `GET /api/library/info` | Done | `lib.rs:438` | local |
| `GET /api/library/history` | Done | The app's recent-libraries list through `Handle::set_history` | local |
| `POST /api/library/switch` | Done | `lib.rs:450-461`; the app follows it (`app.rs:692-722`) | local |
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
| Install a plugin by opening a `.eagleplugin` file | Missing | — | local |
| Open With: list the apps that can open a file | Done | `crates/falcon-app/src/open_with.rs` | local |
| AppleScript dictionary or macOS Services | — (not counted) | — | — |
| Browser extension (Chrome, Safari and others) sending images, video, bookmarks and batch saves to the app on port 41595 | Missing | Falcon never uses 41595, so the stock extension cannot reach it; bookmarks and batch save are also missing | online |
| Plugin system: window, inspector, format (thumbnail and preview) and background-service plugins | Missing | — | local |
| Plugin Center (browse, install, update, disable or uninstall; sort and search) | Missing | — | online/plugin |
| Plugin development (create from template, import a local project, pack, publish, reload, DevTools) | Missing | — | local |
| Actions (recorded multi-step automations with shortcuts) | Missing | — | local |
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
| Eight UI languages: English, Simplified Chinese, Traditional Chinese, Japanese, Korean, Spanish, German and Russian (picked automatically from the system language, or set in Preferences) | `i18n/index.js` `supportLanguage`; `i18n/{en,zh_CN,zh_TW,ja_JP,ko_KR,es_ES,de_DE,ru_RU}.js`; collect window `locales/*.json`; PH `general.language` | Partial | `crate::locale` with falcon's own catalogues for all eight (`crates/falcon-app/locales/`, machine-written, unreviewed), picked from the system language or set in Preferences, applied live incl. the menu bar; the viewers, sidebar, context menus and the Auto-Import/Developer blocks are still English | local |
| Chinese pinyin search support (jieba dictionaries) | Missing | Model round-trips `pinyin` only | local |

---

### Counts

Rows marked "not counted" are features Eagle 4.0.0 does not have.

- **Done:** 175, including 5 where falcon goes beyond Eagle and 1 with a small difference.
- **Partial:** 12.
- **Missing:** 48, including 3 left out on purpose (`script/inject`, `/api/check`, licensing).
- **Different on purpose:** 1 (the API listens on localhost only).

### Ranked Missing and Partial local items

Ranked by how often a user would hit the gap, and the risk if they do.

1. **Dropping a `.library` folder imports its insides as items.** It should open the library or be refused. This is a data hazard (`app.rs:864-878`).
2. **Folder import keeps no structure.** Dropped folders are flattened into the current folder, and there is no "Import Folders" dialog.
3. ~~No duplicate check on import.~~ Fixed.
4. **Auto-backup and restore are not wired into the app.** Backups happen only through the CLI, and the app opens with `Library::open` rather than `open_or_restore`, so a damaged `metadata.json` makes the library unusable.
5. **No Export to Computer for items or folders.** The Export button is disabled, and overwrite prompts are missing.
6. **No external-change reload or sync-conflict detection.** Nothing watches `metadata.json` or `mtime.json`. This is risky with Dropbox, iCloud or a second app, including Eagle itself.
7. **The import error panel is missing.** Falcon shows a count only, with no per-file reasons, retry or copy-out.
8. ~~Watched auto-import folder is missing.~~ Fixed.
9. ~~Folder auto-tags are not applied on import or paste.~~ Fixed.
10. **`.eaglepack` import and export are missing from the app** (CLI only). Smart-folder packs are missing entirely, and dropping a pack imports the pack file itself as an item.
11. **Merge another library and Add to Other Library are missing.**
12. **Screen capture is missing**: area, window and full screen, global shortcuts, and the whole Screenshot pane.
13. **The copy variants are missing**: name, folder path, thumbnail, base64, Eagle link and source URL. Copy File Path is context-menu only, for one item.
14. **Export to CSV is missing.**
15. **The `eagle://` scheme and document types are not registered.** Opening items, folders or smart folders by link, opening packs by double-click, and "Open With Falcon" from Finder all fail.
16. **Unsupported formats are imported silently** instead of being refused with a message.
17. **The collect window is thin**: no multi-folder, recent folders, new folder, name, description or tags.
18. **Library list management is missing**: clear history, remove from list, library icon, and the missing-library retry or relocate screen.
19. ~~App password lock has no Touch ID.~~ Fixed; Touch ID on locked folders is still missing.
20. ~~Missing API routes.~~ Fixed.
21. **Hard-coded behaviours need preference switches**: font style tag, video autoplay, loop short clips, hover video preview, saved playback position, sidebar counts.
22. **Missing Controls and Preview settings**: middle button, and the image settings not yet wired (see §6).
23. **Sidebar pane is missing**: show or hide sections, and double-click to rename or collapse.
24. **Notifications and sounds are missing**: system pop-ups and sound effects per event, and the Notification Center.
25. **General settings are missing**: language, UI zoom, vibrancy, launch at login, Dock icon, menu bar icon, GPU, auto-select after import.
26. **Localisation**: English only, against Eagle's eight languages.
27. **Library safety checks are missing**: newer-version refusal, warnings for Desktop or Documents paths, read-only paths and slow disks, disk-space checks, and confirmation for large folders or deep trees.
28. ~~"Empty cache and reload" is missing.~~ Fixed; "Clear Invalid Content" is not built (damaged items are skipped and reported).
29. **Smaller gaps**: library loading progress, welcome flow, Preferences search, remembering the last pane, the Share sheet, WebP conversion, saving the current video frame, the menu bar icon, Inboard and Pixave importers, the API playground, debug report and error logs, the plugin system and plugin development, and colour-space selection.
