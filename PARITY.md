# Parity with Eagle

This is the checklist of everything Eagle 4.0.0 can do, compared with falcon. It was built on 2026-11-01 from Eagle's shipped app code and its English UI text, and every falcon status was checked in falcon's code, not taken from ROADMAP or the docs. Local parity means every row marked local is Done. Rows marked online or plugin need a network service, an account or Eagle's plugin system, and don't count.

Statuses: **Done** works as in Eagle. **Partial** exists but is missing options or behaves differently. **Missing** isn't there.

| Area | Done | Partial | Missing (local) | Missing (online/plugin) |
|---|---|---|---|---|
| Main window: menus, shortcuts, toolbar, sidebar, layouts | 144 | 40 | 87 | 9 |
| Viewers and file formats | 108 | 26 | 56 | 7 |
| Search, filters and organising | 140 | 29 | 53 | 3 |
| Import, export, library, preferences and integrations | 111 | 19 | 82 | 26 |
| **Total** | 503 | 114 | 278 | 45 |
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
9. No tag picker or suggestions.
10. ~~No GIF player controls, no animated WebP, no GIF hover previews.~~ Fixed; still no animated AVIF (no decoder) and no hover playback for video or audio.
11. No interactive 3D viewer, text editor or full font viewer.
12. No watched import folder, no reload on outside changes, no sync-conflict detection, no screen capture.
13. English only; Eagle ships 8 languages.

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
| App menu: About | Partial | Only winit's stock About panel. No Eagle-style About window | local |
| App menu: Preferences… (⌘,) | Done | shortcuts.rs `app.preferences`; in the app menu after About (`menu.rs`) and in the library switcher | local |
| App menu: Lock Now (⌘⌥L, locks the whole app behind a password or Touch ID) | Partial | ⌥⌘L locks the app behind the password set in Preferences ▸ Password (`applock.rs`, `app.lock`); in the app menu. No Touch ID | local |
| App menu: Color Space (sRGB / Display P3 / Unmanaged) | Missing | No colour-space choice | local |
| App menu: Hide, Hide Others, Quit | Done | Supplied by winit's default macOS app menu (main.rs uses eframe defaults) | local |
| App menu: Check for Updates, Register / Unregister, Manage devices | Missing | Not applicable to falcon | online/plugin |
| File: New… (⌘N), a searchable menu of folder, smart folder, imports, duplicates and new documents from templates (Sketch, PSD, AI, XD, TXT, Word, PPT, Excel, Keynote, Pages, Numbers, MindNode, XMind) | Missing | No New… menu and no create-from-template | local |
| File: New Folder (⇧⌘N) | Done | A sibling right after the open folder, or top level when none is open, as Eagle's `newFolder` (`Action::CreateSibling`) | local |
| File: New Subfolder (⌥N) | Done | shortcuts.rs `file.create.subfolder`; menu and folder context menu | local |
| File: New Smart Folder (⇧⌥⌘N) | Done | shortcuts.rs; opens the smart folder dialog | local |
| Edit: Undo / Redo | Done | ⌘Z and ⇧⌘Z with a capped transaction history; Edit menu entries go to a focused text field first | local |
| Edit: Cut / Copy / Paste / Select All | Partial | Menu entries exist and act on a focused text field; ⌘C copies files, ⌘V imports, ⌘A selects all. Cut works only in text fields | local |
| Edit: Rename (⌘R on macOS) | Partial | ⌘R, with F2 as an alias (shortcuts.rs `edit.rename`). It only focuses the inspector title for one item. Eagle renames several items in a batch dialog | local |
| Edit: Copy File Path (⌥⌘C) | Done | Key and item menu copy every selected path, one per line (`export_ui.rs` `copy_variant`) | local |
| Edit: Copy Folder Path (⌥⇧⌘C), Copy Eagle Link, Copy Thumbnail, Copy Base64, Copy Name | Partial | All but Copy Eagle Link (`export_ui.rs` `item_copy_menu`); Copy Folder Path copies the items' `.info` folders as Eagle does. Falcon doesn't register `eagle://` | local |
| Edit: Duplicate item (⌘D) | Missing | No duplicate-item action | local |
| Edit: Refresh Thumbnail (⌥⌘R), Custom thumbnail from a file (⌥⌘T), Reset custom thumbnail | Partial | ⌥⌘R and its menu entry (`run_command`); custom thumbnail from a file is context-menu only, no ⌥⌘T | local |
| Edit: Custom thumbnail from the clipboard (⇧⌥⌘T) | Missing | Choosing a file is the only option | local |
| Edit ▸ Move Folder: Up, Down, To Top, To Bottom (⌘] ⌘[ ⇧⌘] ⇧⌘[) | Done | Keys and Edit menu entries move the open folder or smart folder among its siblings, undoably (`organize::arrange`) | local |
| Edit ▸ Edit Image: Horizontal Flip (⇧F), Rotate (⇧R), Crop (⇧C), Combine Images (⇧⌘M) | Partial | Edit ▸ Edit Image menu has Flip, Rotate and Crop; ⇧R and ⇧F work in the viewer. No ⇧C key and no Combine Images | local |
| Edit ▸ Edit Folder: Set Current Folder (⇧⌘R), Move Folder, Password create / edit / remove / lock (⇧⌘L) | Partial | ⇧⌘R opens Folder Settings; Edit ▸ Move Folder… opens the picker dialog (`Command::MoveFolder`, no default key as Eagle). Passwords are still context-menu only, no ⇧⌘L | local |
| Edit: Remove from current folder (⇧⌘⌫) | Done | shortcuts.rs `edit.remove.folder`, menu and card menu; one undo step, no prompt (Eagle's `removeFromFolder` asks nothing) | local |
| Edit: Move to Trash (⌘⌫) | Done | shortcuts.rs:114, app.rs:1095-1102 | local |
| Find: Go to Folder (⌘J or J), a quick-switch dialog with Folders / Smart Folders / Tags tabs | Missing | No quick switcher | local |
| Find: Add to Folders… (⇧⌘J, a searchable folder picker that can create folders) | Partial | A plain scrolling submenu in the item context menu (app.rs:2520). No search, no new folder and no key | local |
| Organize: Add To Last Used Folder (⇧D) | Missing | No record of recently used folders | local |
| Organize: Rate 0–5 stars from the menu and keys | Done | Organize menu and the 0–5 keys (`organize.rating.*`) with a toast; in the viewer the bare digits keep their zoom meaning | local |
| Organize: Add Tags… (⌘T), Copy Tags (⇧⌘C), Paste Tags (⇧⌘V), Clear Tags | Missing | No tag-panel key and no tag clipboard | local |
| View: Always on Top (⇧T) | Missing | Only the collect window floats (collect.rs:89). The main window cannot | local |
| View: jump to All / Uncategorized / Untagged / Recent / Random / All Tags / Trash (⌘1–⌘7) | Done | shortcuts.rs `view.all` … in Eagle's View menu order; from the viewer they close it first | local |
| View: layout Waterfall / Justified / Grid / List with checkmarks (⌥1–⌥4) | Done | ⌥1–⌥4 in Eagle's order; View menu entries with checkmarks (`menu.rs` `validateMenuItem`) | local |
| View: Scroll to Top / Bottom / Page Up / Page Down (Home, End, PgUp, PgDn) | Done | Home, End, Page Up and Page Down scroll the grid (a page is the view less 72 px, as Eagle); ⌘↑ / ⌘↓ select the first or last item | local |
| View: Zoom In / Out (⌘= / ⌘-) | Done | Thumbnail size (shortcuts.rs:96-97, app.rs:1024-1029) | local |
| View: Actual Size (⌘0) / Zoom to Fit (⌘9) | Partial | The viewer has 0 for fit and 1 for actual size, without ⌘ (app.rs:3510). No ⌘0 or ⌘9 | local |
| View: Grayscale preview (⌥⌘G) | Missing | No grayscale toggle | local |
| View: Open in New Window (⌘O) | Missing | See §9 | local |
| View: Open With Default App (⇧↩), Open with Other App, Reveal in Finder (⌘↩) | Done | open_ui.rs:33-46, shortcuts.rs:104-105 | local |
| View: Open Link in Browser (⇧⌘O) | Missing | No key or menu item opens the item's source URL | local |
| View: show or hide Sidebar (⌥⌘1), Inspector (⌥⌘2), both (⌥⌘3) | Done | shortcuts.rs `view.toggle.sidebar`, `view.toggle.inspector`, `view.toggle.all`; Tab also toggles both; all work in the viewer too | local |
| View: Show Name (⌥⌘4), Show Item Info (⌥⌘5), Show Comment count (⌥⌘6) | Done | shortcuts.rs `view.toggle.listname/listmetas/listannotation`, View menu checkmarks, `logic::Display` | local |
| View: Show Subfolder Content (⌥⌘7) | Done | `view.toggle.subfolder`, `logic::folder_members`, `grid_order` in organize_ui.rs; the layout panel and the strip heading have the switch too | local |
| View: Enter / Leave Presentation Mode (F5) | Missing | No slideshow | local |
| Actions menu: user-defined Actions, each with its own shortcut | Missing | The toolbar ⚡ button is drawn but disabled (app.rs:2038) | local |
| Window: Toggle Full Screen, Minimize, Close Window (⌘W) | Done | ⌃⌘F, ⌘M and ⌘W through eframe viewport commands; ⌘W closes the viewer first, then the window (which quits falcon, like the red button) | local |
| Help: Help Center, Show Tutorial, Install Extensions, Feedback, Recover license, Change Logs | Missing | Not applicable | online/plugin |
| Help: Error Logs, Export Debug Report… | Missing | No log viewer or debug report export | local |

Cross-references (not counted): File ▸ Import Folders / Links / Eagle Package / Pinterest / ArtStation / Auto Import, File ▸ Export to Computer / .eaglepack / Specified Format / CSV, Library menu (Create / Switch / Merge / Clear History / Empty cache and reload) and Find ▸ Search / Search All / Filter submenu / Search by Image / Find Duplicates belong to the import, export, library and search audits. Falcon's library switcher popup (app.rs:1595-1652) covers Open, Recent, New, Show in Finder, Collect Window and Preferences.

---

### 2. Context menus

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Every context menu has a type-to-filter search box with keyword matching | Missing | Plain egui context menus | local |
| **Item**: Restore (in the Trash) | Done | app.rs:2531-2534, which also offers Delete Permanently | local |
| Item: Open Item(s) in New Window | Missing |  | local |
| Item: Open With Default App / Open With ▸ apps / Reveal in Finder | Done | open_ui.rs:33-46 | local |
| Item: Open File Location ▸ All or one of the item's folders (jumps to the item there) | Missing |  | local |
| Item: Plugin ▸ recent plugins | Missing |  | online/plugin |
| Item: Add To Last Used Folder | Missing |  | local |
| Item: Add to Folders… (picker panel) | Partial | Flat list submenu (app.rs:2520-2528) | local |
| Item: Share (macOS share sheet) | Missing |  | local |
| Item: Pin to Top / Unpin from Top inside a folder | Missing | No pinning | local |
| Item: Set as Folder Cover | Done | image_edit.rs:90-95 | local |
| Item: New Folder with Selection (N items) | Done | `folders_ui` `new_with_dialog`: creates the folder and adds the selected items | local |
| Item: Rename / Rename N Items (batch rename dialog) | Missing | Not in the context menu. Only F2 on one item via the inspector | local |
| Item: Copy (files) | Partial | ⌘C works (app.rs:942). Not in the menu | local |
| Item: Copy File Path | Done | `export_ui.rs` `copy_variant`, every selected item | local |
| Item: Copy… ▸ Link, Folder path, Thumbnail, Base64, Name | Done | `export_ui.rs` `item_copy_menu`; Link copies the item's source URL | local |
| Item: Copy Tags / Paste Tags | Missing |  | local |
| Item: Combine Images (several selected) | Missing |  | local |
| Item: Duplicate | Missing |  | local |
| Item: Search by Image ▸ Google / Bing / Yandex / TinEye / SauceNAO | Missing |  | online/plugin |
| Item: Enter / Leave Presentation Mode | Missing |  | local |
| Item: Thumbnail Background ▸ None / Black / Gray / White / Checkerboard grid | Missing |  | local |
| Item: Grayscale preview toggle | Missing |  | local |
| Item: More ▸ custom thumbnail (file) / reset | Done | image_edit.rs:96-103 | local |
| Item: More ▸ custom thumbnail from the clipboard | Missing |  | local |
| Item: More ▸ Refresh Thumbnail | Done | image_edit.rs:104 | local |
| Item: More ▸ Re-analyze colors | Done | Labelled "Refresh Palette" (image_edit.rs:107) | local |
| Item: More ▸ Replace with File… | Missing |  | local |
| Item: Remove from this Folder | Done | Card menu entry and ⇧⌘⌫ | local |
| Item: Move to Trash | Done | app.rs:2538 | local |
| Item (falcon only): Rotate Left / Right, Flip H / V, Crop… | Done | image_edit.rs:110-132 (falcon extra) | local |
| **Folder**: New Folder (sibling) | Done | `Action::CreateSibling`, right after the clicked folder under the same parent | local |
| Folder: New Subfolder | Done | app.rs:1836 | local |
| Folder: Move Folder… (picker dialog) | Done | `folders_ui` `move_dialog` with destination search; `Library::move_folders` (above, into or below a target) | local |
| Folder: Add to / Remove from Quick Access | Done | app.rs:1842-1848 | local |
| Folder: Rename | Done | Inline rename (app.rs:1839, 1797-1823) | local |
| Folder: Copy Link (eagle:// folder link) | Missing |  | local |
| Folder: Set Auto-Tag | Done | "Auto-Tags…" in the folder menu and ⇧⌘R (`organize_ui.rs` auto_tag_dialog) | local |
| Folder: Password ▸ Create / Edit / Remove / Lock Now | Done | organize_ui.rs:270-296 | local |
| Folder: Sort By ▸ Title A-Z / Z-A, current level or all levels | Done | organize_ui.rs:327-336 (falcon asks to confirm, like Eagle) | local |
| Folder: Expand/Collapse ▸ this folder, same level, all folders | Missing | Caret click and double-click only | local |
| Folder: Duplicate | Done | `Library::duplicate_folder`: copy placed after the original, new ids, no password, no items | local |
| Folder: Show Subfolder Content | Missing |  | local |
| Folder: Change Icon… and colour row | Done | organize_ui.rs:308-320 | local |
| Folder: item sort for this folder (Order By) | Done | "Sort Items By" (organize_ui.rs:300) | local |
| Folder: Remove Folder | Done | Asks only when the folder has items or subfolders, with a ticked "trash the items too" box that can be unticked; disabled while locked | local |
| **Several folders selected**: menu with batch rename, shared order, move, quick access, export, icon, colour, remove | Done | `multi_folder_menu` in `folders_ui`: batch rename (Eagle's templates, `rename.rs`), shared sort, Move Folder…, Quick Access, Export to Computer, icon and colour for all, remove. Falcon's icon grid instead of emoji | local |
| **Smart folder**: New Smart Folder (sibling) / New Subfolder / Rename / Edit Rules / Quick Access / Change Icon / Remove | Done | organize_ui.rs:372-423 | local |
| Smart folder: Duplicate | Done | `smart::duplicate_smart_folder`: deep copy placed before the original, `parent` keys fixed | local |
| Smart folder: Refresh | Missing | Smart folders refresh live, so this matters less | local |
| **Smart Folders heading +**: New Smart Folder / New Group | Done | organize_ui.rs:488-505 | local |
| **Quick Access item**: Remove from Quick Access | Done | app.rs:1692-1696 | local |
| **Subfolder card** (strip above a folder's items): Rename / Auto-Tag / Password / Export / Remove | Missing | The cards have no menu (covers.rs:65-140) | local |
| **Trash** sidebar item: Empty Trash | Done | app.rs:1758-1763 | local |
| Trash sidebar item: Restore All Items | Missing |  | local |
| **Recently Used** item: Clear history | Missing |  | local |
| **Sidebar nav area**: choose which entries are visible (All, Uncategorized, Untagged, Recent, Random, Community, All Tags) | Missing |  | local |
| **Sidebar / panel edges**: Toggle Sidebar / Inspector / both | Missing | Buttons only (app.rs:1585, 3644) | local |
| **Folders heading**: Sort folders by title | Done | app.rs:1713-1715 | local |
| **Empty grid** right-click opens the layout and sort panel | Missing | The background only clears the selection (app.rs:2490-2493) | local |
| **List-view column header** right-click picks columns (tags, dimensions, rating, extension, file size, date added) | Done | `layout_panel::list_header` | local |
| **Toolbar title** right-click lists the ancestor folders to jump to | Missing |  | local |
| **Tag** (All Tags page): Rename / Star / Move to Group / Remove from Group / Remove | Done | app.rs:2849-2861, organize_ui.rs:919-940 | local |
| Tag: Filter items with the tag(s), Merge N tags (several selected), Add to several groups | Partial | Clicking a tag opens its items (app.rs:2845) and renaming onto an existing tag merges. No multi-select merge and no menu entry | local |
| **Tag group**: Rename / Remove | Done | organize_ui.rs:820-845, which adds description, move and colour | local |
| Tag group: Find items with any tag in the group | Missing |  | local |
| **Tag list sort**: A>Z / Z>A / by count / group by Alphabet or Groups | Partial | Group-by toggle only (organize_ui.rs:722-730) | local |
| **Palette swatch** (inspector): Find similar colour, copy as HEX / RGB / RGBA / HSL / HSV / HWB / CMYK, set as primary | Missing | Not found in the inspector code | local |
| **Inspector tag input**: Copy / Paste / Clear All Tags | Missing |  | local |

---

### 3. Keyboard shortcuts

Eagle defaults come from prefs `shortcuts.keybinds` (rebindable) and from the hard-coded Mousetrap table at bundle 49291-49340.

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Shortcut editor: rebind, conflict check, clear, reset, grouped list | Done | shortcuts.rs:150-200, 400-530 | local |
| Shortcut editor: search field ("Search Shortcut") | Missing | No filter in the shortcut list | local |
| Shortcut editor coverage: the roughly 150 rebindable commands | Partial | 60 commands plus 12 player keys (shortcuts.rs TABLE) | local |
| ⌘F search, ⇧⌘F toggle filter | Done | shortcuts.rs:108-109 | local |
| ⌥⌘F search all, ⌥⇧F filter sidebar, ⇧⌥⌘F reset filter, ⌥F / ⌥T / ⌥C / ⌥S / ⌥R / ⌥D / ⌥E open one filter | Missing |  | local |
| ⌘L switch library | Done | shortcuts.rs:117, app.rs:998 | local |
| ⌘E export, ⇧⌘E export .eaglepack, ⇧E export as format | Done | shortcuts.rs Export group | local |
| ⌘1–⌘7 sidebar views | Done | shortcuts.rs `view.all`, `view.unfiled`, … | local |
| ⌥1–⌥4 layouts | Done | shortcuts.rs `view.layout.*`, in Eagle's order | local |
| ⌥⌘1–⌥⌘8 panel and caption toggles plus Navigator | Partial | ⌥⌘1–⌥⌘7; no Navigator | local |
| ⇧T always on top, F5 slideshow, ⌥⌘G grayscale | Missing |  | local |
| ⌘O open in new window, ⇧⌘O open link | Missing |  | local |
| ⇧↩ default app, ⌘↩ reveal | Done | shortcuts.rs:104-105 | local |
| ⌥⌘S reveal, ⌥⌘D duplicate (view) | Missing |  | local |
| Home / End / PageUp / PageDown scroll the grid; ⌘↑ / ⌘↓ jump to top or bottom | Done | `run_command` scroll offsets; ⌘↑ / ⌘↓ also select the first or last item, where Eagle only scrolls | local |
| 0–5 set the rating of the selection | Done | shortcuts.rs `organize.rating.*`, grid only; toast "Rated N stars" | local |
| T opens the tag panel, F opens the folder panel, J opens Go to Folder, G opens Actions, P opens Plugins | Missing |  | local |
| R reshuffles the Random view | Done | shortcuts.rs; in place, without a history step | local |
| `*` or `/` expand or collapse every folder | Done | Eagle's `toggleAllFolders` rule (smart folders when one is open); `*` is the ⇧8 alias chord | local |
| Tab toggles all panels | Done | Fixed key in shortcuts.rs FIXED, when no text field has focus (`logic::toggle_all`) | local |
| Space opens the quick preview (preference: preview, native Quick Look, or page down) | Done | `Prefs::space`: viewer, Quick Look (`open_with::quick_look`, `qlmanage -p`) or page down | local |
| ⇧Space pages up | Missing |  | local |
| Enter toggles the viewer | Done | Enter is an alias of Viewer (shortcuts.rs:95) | local |
| Esc closes panel, slideshow or viewer, otherwise clears the selection | Done | app.rs:1076-1081 | local |
| Arrow keys move the selection; ←→ step in the viewer | Done | app.rs:1105-1135 | local |
| WASD as arrow keys | Missing |  | local |
| ⇧ + arrows extend the selection | Done | `logic::extend_selection`: ⇧←/→ grow or shrink from the anchor; ⇧↑/↓ only in List layout, as Eagle | local |
| Arrow keys move through the sidebar tree when it has focus (focus zones: content / sidebar / tags) | Missing |  | local |
| ⌫ goes back (or leaves the viewer); ⌥← / ⌥→ and ⌘← / ⌘→ move through history | Done | shortcuts.rs `go.back` (⌫, ⌘←, ⌥←) and `go.forward` (⌘→, ⌥→); in the viewer ⌫ leaves it (app.rs `keyboard`) | local |
| ⌥↑ opens the parent folder | Done | Of the open folder or smart folder | local |
| ⌥⇧N creates a text file from a template; ⌥⇧C sets the folder cover | Missing | Folder cover is in the context menu only | local |
| ⌘W closes the window | Done | Closes the viewer first, then the window | local |
| ⌘Z undo | Done | app.rs:992 | local |
| ⌘A select all, ⌘C copy | Done | shortcuts.rs:110-112 | local |
| Delete (Windows) / ⌘⌫ trash; ⇧⌘⌫ remove from folder | Done | shortcuts.rs `edit.remove.trash`, `edit.remove.folder` | local |
| ⌘T add tags, ⇧⌘C / ⇧⌘V copy and paste tags, ⇧D add to last folder, ⌘D duplicate | Missing |  | local |
| ⇧⌘L lock the folder now, ⌥⌘L lock the app | Partial | ⌥⌘L locks the app (`app.lock`); no ⇧⌘L for the folder | local |
| Hold Z for the hover preview | Done | app.rs:2703, hover.rs | local |
| Global shortcuts: Show Eagle, Bring up Eagle and search, screen captures (⌥⌘E) | Missing | Captures cross-ref import | local |
| Falcon extras: ⇧⌘K collect window, ⌃Tab / ⌃⇧Tab next or previous open library | Done | shortcuts.rs `file.collect.window`, `library.next`, `library.previous` (falcon only) | local |

Viewer and player keys (frames, volume, speed, ` zoom toggle, N comment, M mute, C comment mode) belong to the viewer audit. Falcon has the player table at shortcuts.rs:122-134.

---

### 4. Toolbar

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Show-sidebar button when the sidebar is hidden | Done | app.rs:2006-2012 | local |
| Back / Forward buttons | Done | app.rs:2013-2018 | local |
| Title for the current view, "Search results", or "N folders selected" | Done | `logic::TreeSel`; the title reads "N folders selected" while several are picked | local |
| Right-click the title for an ancestor jump menu | Missing |  | local |
| Thumbnail size slider with − and + | Done | app.rs:2085-2109 | local |
| Refresh button in the Random view | Missing |  | local |
| Layout and sort panel button | Done | `app/layout_panel.rs` in Eagle's order (layout, Fit/Fill, sort, Show Name/Info/Extension/Label/Comments/Subfolders, sidebar and inspector, Refresh); no Speed/Quality | local |
| Filter button with an active state | Done | app.rs:2034, 2043-2049 | local |
| Search field with a scope menu (Tags / URL / Comment / Note) and a search-by-image camera | Partial | Field works (app.rs:2061). The ▾ and 📷 are decoration only, the camera is disabled. Cross-ref search | local |
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
| Recently Used lists items the user recently opened | Partial | Falcon sorts by last modified (logic.rs:170), not by what was opened | local |
| Community entry | Missing |  | online/plugin |
| Show or hide each nav entry (context menu and preferences) | Partial | Preferences ▸ Sidebar (`SidebarSections`); no context menu | local |
| Show or hide the count badges | Done | `Prefs::sidebar_counts` | local |
| Quick Access section | Done | app.rs:1670-1699 | local |
| Section headers (Quick Access / Smart Folders / Folders) collapse on click and show a count | Partial | Folders shows a count (app.rs:1704). The headers do not collapse (app.rs:1931) | local |
| Smart Folders tree with groups, nesting, icons, colours, counts | Done | organize_ui.rs:340-450 | local |
| Folder tree with carets, guide lines, icon and colour tints, counts, lock badge | Done | app.rs:1778-1890 | local |
| Double-click a folder to expand or collapse (preference: or rename) | Done | `Prefs::sidebar_double_click`, folders and smart folders | local |
| Select several folders with ⌘ or ⇧ click to see their combined items | Done | `logic::TreeSel` (⌘ toggles, ⇧ ranges among visible rows), `folders_ui`; the grid shows the union under the global sort | local |
| Filter field at the bottom of the sidebar | Done | app.rs:1656, 1719-1736. No ⌥⇧F key | local |
| "Go to Folder" button | Missing |  | local |
| Resize the sidebar by dragging its edge | Missing | Fixed width (app.rs:3624-3631) | local |
| Background task status: "Processing" or "Paused" palette analysis with Pause / Resume | Missing |  | local |
| Drop zone hint: "Drag and drop folder here to import" | Missing |  | local |

---

### 6. Grid layouts and display options

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Four layouts: Waterfall, Justified, Grid, List | Done | logic.rs:224-262 | local |
| Default layout is Justified | Done | `logic::Layout` default, `logic::layout_for` | local |
| Layout remembered per folder | Done | `logic::layout_key`: per folder or smart folder, else per library (`Saved.layouts`), as Eagle's `eagle.list.layout.<id>` | local |
| Grid layout Fit or Fill option | Done | `Display.fill`, `draw_image_cover`; hover playback still works in Fill | local |
| Thumbnail Speed vs Quality (load originals for large tiles) | Missing |  | local |
| Sort-by choices in the layout panel, global and per folder, including Manual, Duration and Random | Partial | Global sort lacks Duration, Random and Manual (search.rs:1026). Per-folder sort exists (organize_ui.rs:563-660). Cross-ref search | local |
| Toggle Show Name under thumbnails | Done | `Display.name`, default on | local |
| Toggle item info, choosing Dimensions / File Size / Tags / Rating / Date Modified / Date Created | Done | `logic::caption_info` with Eagle's six types and fallbacks (duration for media, size without preview, dash for no tags, stars, import time for dates) | local |
| Toggle the file extension in the name | Done | `logic::caption_name` | local |
| Toggle the extension label badge | Done | `Display.ext_badge` | local |
| Toggle the comment-count badge | Done | `Display.comments` | local |
| Show subfolder contents in a folder | Done | `logic::folder_members`; search and filters widen too; the strip lists every descendant and manual drag is off | local |
| Show sidebar and Show inspector switches in the layout panel | Done | `layout_panel.rs` | local |
| Refresh button in the layout panel | Done | Flushes, re-reads the library from disk, forgets textures and reshuffles | local |
| List layout with configurable columns and click-to-sort headers | Done | `layout_panel::list_header`, `logic::ListColumns`, `header_sort`: Eagle's seven columns, click sorts (writes the folder's sort inside a folder), right-click picks columns | local |
| Subfolder cards above a folder's items, with covers | Done | covers.rs:65-140 | local |
| Collapse the subfolder strip | Done | Click the strip heading (`Display.strip_collapsed`) | local |
| Rename a subfolder card by double-clicking its name | Done | covers.rs `strip_rename`; a single click on the card body still opens the folder (Eagle selects) | local |
| Thumbnail background (checkerboard for transparency, etc.) | Done | Card menu Thumbnail Background ▸ None/Black/Gray/White/Checkerboard, item key `background` with Eagle's values (`logic::ThumbBg`, `Action::Background`); drawn in grid, list and viewer | local |
| Hover zoom button and hold-Z preview | Done | app.rs:2680-2743, hover.rs | local |
| Video, GIF and audio play on hover in the grid | Partial | GIF and animated WebP cards play in place while hovered (`anim.rs` `Cards`, ⌥ keeps them still); video and audio don't | local |
| Pinned items stay at the top of a folder | Missing |  | local |
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
| Drag items onto a sidebar folder | Done | `logic::folder_drop`: inside a folder the drop moves the items out of it, ⌥ adds; from other views it adds; trashed items come back (`Action::DropOnFolder`, one undo step). A locked target isn't asked for its password | local |
| Toast after a drop ("N items moved to Folder", with a link to the folder) | Partial | `logic::folder_drop_notice` shows "Moved N items to Folder" / "Added …". No link; falcon's toast has no actions | local |
| Drag items onto a Quick Access folder | Missing | Quick Access rows take no drops (app.rs:1688-1697) | local |
| Drag items onto a subfolder card | Missing |  | local |
| Drag items onto the Trash | Done | app.rs:1764-1767 | local |
| Drag folders to reorder or nest, with insertion lines | Done | organize_ui.rs:254-266 | local |
| Drag a folder onto the Folders heading to move it to the top level | Done | app.rs:1705-1711 | local |
| Drag smart folders to reorder or nest | Done | organize_ui.rs:362-372 | local |
| Drag Quick Access entries to reorder them | Missing |  | local |
| Drag items within a manually sorted folder to reorder them | Done | organize_ui.rs:616 | local |
| Drag items out to Finder or other apps | Done | app.rs:3699-3730 | local |
| Drag files from Finder onto the grid to import into the open folder | Done | app.rs:3601-3606, 852-862 | local |
| Drag files from Finder onto a specific sidebar folder | Done | `import_ui.rs` `drop_files` hit-tests the drop against folder rows (`drop_pos.rs`); files go into that folder, folders are ignored, as Eagle's `onDropFolder` | local |
| Drag a Finder folder onto the sidebar to import it with its folder tree | Done | `folder_import.rs` plan and `import_ui.rs` `import_folder_trees`: a drop on the sidebar's empty space makes a top-level folder tree and opens it; on the grid it imports flat, as Eagle's `onDropContainer` does | local |
| Drop an .eaglepack onto the window | Missing | Cross-ref import | local |
| Drop an image or URL from a browser | Missing |  | online/plugin |
| Drag tags onto tag groups or the starred list; ⌥ copies instead of moving | Missing | Cross-ref organising | local |
| Drag ghost showing the item count | Done | app.rs:3732-3740 | local |

---

### 9. Window behaviours

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Frameless window with traffic lights; drag the title area; double-click to zoom | Done | main.rs:45-51, app.rs:1510-1519 | local |
| Remember window size and position | Done | eframe persistence (Cargo.toml `persistence` feature) | local |
| Full screen | Partial | Through the OS green button only. No menu or key | local |
| Always on top for the main window | Missing |  | local |
| Open items in a separate preview window: own titlebar always-on-top toggle, opacity 10–100%, navigator, zoom presets 5–800%, grayscale, rotate / flip, copy, copy path / link, Reveal in Eagle | Missing |  | local |
| Middle-click opens an item in a new window (preference) | Missing |  | local |
| Presentation / slideshow mode | Missing |  | local |
| Closing the window keeps the app running (Dock icon stays); reopen from the Dock | Missing | Closing quits | local |
| Menu bar status icon: Show Eagle, Capture Area / Window / Full Screen, Quit | Missing | Captures cross-ref import | local |
| Lock screen for the whole app, with Touch ID | Missing |  | local |
| Loading screen while a library opens (progress messages) | Missing | Opening is synchronous (app.rs:595-618) | local |
| Resizable inspector | Missing | Fixed width (app.rs:3633-3640) | local |
| Several libraries open in one window (falcon only) | Done | app.rs:624-676, workspaces.rs (extra) | local |
| Floating collect window (falcon only) | Done | collect.rs (extra) | local |

---

### 10. Themes and appearance

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Themes: Auto, Light, Light Grey, Gray, Dark, Blue, Purple | Done | theme.rs:73-130, prefs.rs:312 | local |
| Auto theme follows the system | Done | theme.rs:365 `follow_system` | local |
| Transparency (vibrancy) effects | Missing |  | local |
| UI zoom level | Missing | Cross-ref preferences | local |
| Interface language | Missing | English only. Cross-ref preferences | local |
| File-type colours on extension badges | Done | theme.rs:447-468 | local |

---

### 11. Status and notification UI

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Toast messages (rename, remove, rate, copy path, pin and so on) | Partial | One transient 3-second toast (app.rs:824, 3675-3686). Many actions raise none | local |
| UNDO button on toasts | Missing | ⌘Z works, but toasts have no button | local |
| Clickable links in toasts (open the folder) | Missing |  | local |
| Import progress bar with count and time remaining | Partial | Count and file name (app.rs:911-940). No time remaining and no cancel | local |
| Import errors toast ("N files failed, click for details") opens an error list with retry and reveal | Partial | Toast shows the failure count only (app.rs:925) | local |
| Dock icon progress bar during long tasks | Missing |  | local |
| Progress modals for export, eaglepack, thumbnail refresh, empty trash, WebP conversion and similar | Partial | Import only. Other tasks run synchronously | local |
| Sound effects (delete item, delete folder, capture) | Missing |  | local |
| System notifications (import done, capture done) | Missing | Cross-ref import | local |
| Warnings: slow disk, NTFS on macOS, library path permissions, localhost / API port | Partial | API port errors appear in Preferences (prefs.rs:100). No disk or permission warnings | local |
| Tooltips on toolbar buttons | Done | app.rs:1492-1507 | local |

---

### 12. Onboarding and empty states

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Welcome flow: welcome, choose a theme, install browser extension, create or open a library with example names | Missing |  | local (extension step online) |
| No-library screen offers both Create and Open | Partial | Only "Choose Library…" (app.rs:3531-3546). Create is reachable from the switcher only after a library is open | local |
| Tutorial / tips overlay | Missing |  | local |
| Empty All or folder view: "Drag and Drop" with a hint | Partial | Plain "No items" for every view (app.rs:2448-2451) | local |
| Empty search: "Nothing Found, maybe another keyword" | Partial | Same "No items" | local |
| Empty Trash text | Partial | Same "No items" | local |
| Uncategorized and Untagged intro and "done" states | Partial | Same "No items" | local |
| No tags yet / no uncategorized tags | Partial | Tag page has no tailored empty text | local |
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
8. Go to Folder quick switcher (⌘J), and Add to Folders as a searchable picker instead of a flat list.
9. No multi-folder selection in the sidebar.
10. Open in New Window (preview window with always-on-top, opacity and navigator), plus main-window Always on Top.
11. Presentation / slideshow mode (F5) and grayscale preview.
12. Folder context menu gaps: New Folder (sibling), Move Folder dialog, Expand/Collapse submenu, Duplicate, Copy Link, Show Subfolder Content. Also no menu at all on subfolder cards, and no menu for several folders.
13. Fixed-width sidebar and inspector (Eagle's are resizable). Sidebar sections don't collapse. Nav entries can't be hidden.
14. ~~List layout: no column header, no column chooser, no sort-by-column.~~ Fixed.
15. Empty states are a single "No items"; no welcome flow; the no-library screen lacks Create Library.
16. Toast system: no UNDO button, no links, no import error list, no dock progress, no time remaining.
17. Drag and drop gaps: files onto a specific sidebar folder, a Finder folder onto the sidebar (keeping its tree), items onto Quick Access or subfolder cards, Quick Access reorder.
18. Context menus have no type-to-filter search.
19. Trash ▸ Restore All, Recently Used ▸ Clear history, and Recently Used tracks modification time rather than items the user opened.
20. Right-click on the empty grid (layout panel), right-click on the title (ancestor jump), ⌥↑ to the parent folder, R to reshuffle Random and a refresh button.
21. Video, GIF and audio hover-play in the grid.
22. Window: closing quits instead of keeping the app in the Dock; no Full Screen menu or key; no app lock screen; no menu bar status icon; no library loading screen.
23. Smaller items: Color Space menu, vibrancy, sound effects, sidebar badge toggle, palette-analysis status indicator, double-click-to-rename preference, shortcut-list search, Error Logs / Debug Report.

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
| 3D model | Inside main window (`index.html:1443`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`), as a still render | Partial |
| Special formats (PSD, AI, Office…) | Inside main window (`index.html:1448`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`), as a QuickLook render | Partial |
| Web page (url/html/mhtml) | Inside main window (`index.html:1491`) | Central panel between sidebar and inspector (`F/app.rs` `fn viewer`, `viewer_ui`), as a still render | Partial |
| Toggle sidebar and inspector from inside the viewer | `toggle-all-btn` in the detail toolbar (`index.html:1015`) | Panels button in every viewer bar and the main toolbar (`App::panels_button`, `logic::toggle_all`), ⌥⌘3 and Tab; both-on/both-off like Eagle | Done |
| Separate preview window | `preview-window.html`, `Context.Open.New.Window` | No second viewer window (only the collect window uses a viewport, `F/collect.rs:84`) | Missing |
| Space quick preview (inline mode) | Inline overlay with prev/next arrows, counter and exit button (`index.html:1242-1254`, `app.bundle.js:33612`) | Space opens the same full viewer (`F/app.rs:1092`) | Partial |

### 1. Detail viewer shell (shared by all viewer types)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Open an item by double-click or Enter | Done | `F/app.rs:2545` | local |
| Preference: double-click opens in the default app instead | Missing | No such preference (`F/prefs.rs:21`) | local |
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
| Presentation (slideshow) mode: full screen, panels hidden | Missing | No full-screen or presentation mode anywhere in falcon | local |
| Always on top | Missing | Only the collect window is always on top (`F/collect.rs:89`) | local |
| Thumbnail background per item (none, black, gray, white, grid) used in the viewer and grid | Done | `paint_thumb_bg` from the item's `background` key; wins over the transparency-grid preference | local |
| Transparent-area display preference (grid or no grid) | Done | `prefs.transparency_grid`, off by default as Eagle | local |
| Grayscale preview toggle | Missing | None (falcon's "grayscale" is only a search filter, `FL/search.rs:487`) | local |
| Navigator (minimap) while zoomed | Missing | None | local |
| Viewer keys follow the shortcut table (zoom, crop, rotate, flip) | Partial | Zoom keys are rebindable (`view.zoom.*`); crop, rotate and flip keys are still fixed | local |

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
| Rotation mode preference (preview only, or write to file) | Partial | Always writes to the file (`FL/transform.rs:65`); no preview-only mode | local |
| Horizontal flip (Shift+F), vertical flip | Done | `F/crop.rs:251-258`, Shift-click flips vertically (`F/image_edit.rs:332`) | local |
| Rotate and flip limited to jpg/png/webp/bmp (flip also avif) | Done | `FL/transform.rs:36-53` | local |
| Crop tool (jpg/png/webp) with handles | Partial | Crop with handles and Save/Cancel (`F/crop.rs:144-232`); no Shift+C key | local |
| Crop width and height number fields | Missing | No numeric entry (`F/crop.rs:203`) | local |
| Crop "Save As" (keeps the original, adds a new item) | Missing | Only "Replace Original" (`F/crop.rs:211`) | local |
| Live crop size readout | Missing | None | local |
| Comment mode: draw rectangle regions with notes (C key) | Done | `F/comments.rs:201-320`, C key `F/image_edit.rs:277` | local |
| Comment badges, delete X, resize handles, count badge on the toolbar button | Done | `F/comments.rs:213-318` | local |
| Hovering a region shows its note | Done | Tooltip on hover (`F/comments.rs:290`) | local |
| Comment count badge on grid cards | Done | `F/comments.rs:180` (always on; no toggle) | local |
| Default viewer mode preference (preview or comment) | Missing | Always opens in preview mode | local |
| Copy, copy path and copy link from the viewer menu | Partial | Copy File Path is in the viewer's right-click menu; no copy or copy link | local |

### 3. GIF and animated images

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| GIF plays in the viewer | Done | `F/anim.rs` streams every frame from disk on a background thread with a bounded frame window (66 MB peak on a 5,000-frame GIF) | local |
| Animated WebP, AVIF and APNG play in the viewer | Partial | WebP and APNG through `anim.rs` `SeqSource`; no AVIF decoder is available offline | local |
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
| Fallback player for formats Chromium can't play (mkv, avi, wmv, flv and others) | Missing | AVFoundation failures fall back to a still image (`F/app/video_view.rs:254`) | local |
| "Can't play this file" page with an Open With button | Partial | Error text in the stage (`F/app/video_view.rs:300`) | local |
| Autoplay on open | Done | `F/app/video_view.rs`; switch `Prefs::video_autoplay` | local |
| Play/pause button, click picture to toggle, Space | Done | `F/app/video_view.rs:309-312`, `:385-392`, `F/shortcuts.rs:122` | local |
| Seek bar with drag scrubbing | Done | `F/app/video_view.rs:320-349` | local |
| Hover thumbnail on the seek bar (preview frame under the pointer) | Missing | None | local |
| Skip back and forward buttons (2%, capped at 10 s / 20 s), hold to repeat | Partial | Buttons and Cmd+Left/Right (`F/app/video_view.rs:394-400`); no hold-to-repeat | local |
| Frame step back and forward, 1 and 10 frames | Done | Buttons and [ ] Shift+[ Shift+] (`F/app/video_view.rs:402-408`, `F/shortcuts.rs:123-126`) | local |
| Playback speed menu 0.25x–8x and Shift+. / Shift+, | Done | `F/app/video_view.rs:441-452`, `F/shortcuts.rs:131-132` | local |
| Loop toggle, remembered | Done | `F/app/video_view.rs:431-439` | local |
| Always loop clips shorter than 30 s | Done | `F/app/video_view.rs`; switch `Prefs::video_loop_short` | local |
| Volume slider, mute, volume keys (Cmd+Up/Down), remembered | Done | `F/app/video_view.rs:412-430`, `F/shortcuts.rs:127-128` | local |
| Volume / seek tip overlay while adjusting | Missing | None | local |
| Wheel over the picture seeks; Shift/Alt+wheel seeks horizontally | Done | `F/app/video_view.rs`; `Prefs::video_scroll` can make the wheel change the volume instead | local |
| Preference: vertical wheel controls volume instead | Missing | No setting | local |
| Remember the last playback position | Done | `Prefs::video_resume`; positions saved per item id in app state (`Positions`, latest 500), across launches | local |
| Stretch video to fill the window | Done | `Prefs::video_fill` | local |
| Full-screen button | Missing | None | local |
| Flip and rotate the video (viewer toolbar) | Missing | None | local |
| Timed notes: add a note at the current time | Done | `F/app/video_view.rs:453-458`, N key `F/shortcuts.rs:134` | local |
| Timed notes shown as markers on the seek bar; click to jump, edit, delete | Done | `F/app/video_view.rs:351-381` | local |
| Set current frame as the video thumbnail | Missing | None | local |
| Copy current frame to the clipboard | Missing | None | local |
| Save current frame as an image | Missing | None | local |
| Load subtitles (.srt, .vtt) | Missing | None | local |
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
| Volume keys (Cmd+Up/Down) | Missing | Player keys only bind to video (`F/app/video_view.rs:202`) | local |
| Skip back and forward keys (Cmd+Left/Right) | Missing | Same | local |
| Playback speed (menu and Shift+. / Shift+,) | Missing | No speed control in `F/audio.rs` | local |
| m4a playback | Done | `F/audio.rs:48` via rodio mp4 | local |
| BPM and duration in the inspector | Done | `F/app/media_view.rs:306-313` | local |

### 7. Font viewer

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Specimen drawn in the font itself | Done | `F/app/media_view.rs:241-298` | local |
| Editable sample text | Done | `F/app/media_view.rs:262-275` | local |
| Size slider with zoom-in and zoom-out buttons | Partial | Slider 12–200 pt (`F/app/media_view.rs:252`); no buttons, no wheel zoom | local |
| Tabs: Article, Waterfall, Alphabet, Other (info) | Partial | One scrolling page with sample, waterfall and character set (`F/app/media_view.rs:276-296`); no tabs, no info tab | local |
| Colour themes for the specimen (auto, light, dark, purple, yellow) | Missing | None | local |
| Font info panel (PostScript, full name, family, version, designer, maker, description, copyright, trademark, glyph count) | Partial | The inspector shows family, style, PostScript, version and glyphs (`F/app/media_view.rs:334-351`); no designer, maker, description, copyright, trademark | local |
| Rename the file from the PostScript, full or family name | Missing | None | local |
| Activate or deactivate (install) the font | Missing | Falcon can only filter by Eagle's activation state (`F/app/filters_ui.rs:219`) | local |
| Change the sample language | Partial | Sample script chosen automatically from the font's coverage (`F/app/media_view.rs:53-63`); no manual choice | local |
| Number keys rate the font from inside the viewer | Missing | No rating keys in viewers | local |
| Fonts egui can't read (some woff2, CFF) | Partial | Falls back to the image viewer (`F/app/media_view.rs:88-90`, `:189`) | local |

### 8. Text (.txt) editor

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| View text in the app | Partial | Shows a QuickLook still render (`F/loader.rs:56-57`) | local |
| Edit the text in place, auto-saved | Missing | None | local |
| Edit the title (rename) at the top of the page | Missing | None | local |
| Zoom the text size (wheel and buttons) | Missing | Zooms the still image only | local |
| Reloads when the file changes outside | Missing | None | local |
| Links in the text open in the browser | Missing | None | local |
| Number keys rate the item from inside the editor | Missing | None | local |

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
| Highlight the outline entry for the current page | Missing | None | local |
| Sidebar attachments list | Missing | None | local |
| Sidebar layers (optional content) | Missing | None | local |
| Find bar with previous/next, match count | Done | `F/app/pdf_view.rs:1286-1332` | local |
| Find option: match case | Done | `F/app/pdf_view.rs:1310-1312` | local |
| Find option: highlight all | Done | All hits always drawn (`F/app/pdf_view.rs:1059-1064`); no toggle | local |
| Find options: whole words, match diacritics | Missing | None | local |
| Select text and copy it | Done | `F/app/pdf_view.rs:1080-1119`, copy `:946` | local |
| Hand tool (drag to pan) | Missing | Dragging always selects text | local |
| Rotate pages clockwise and counter-clockwise | Missing | Only honours the file's own rotation (`crates/falcon-pdf/src/geom.rs:20`) | local |
| Scroll modes: page, vertical, horizontal, wrapped | Missing | Vertical only | local |
| Spread modes: none, odd, even (two-page) | Missing | None | local |
| Presentation mode (full screen) | Missing | None | local |
| Print | Missing | None | local |
| Document properties dialog | Missing | None | local |
| Password-protected PDFs | Missing | No password prompt in `crates/falcon-pdf/src/mac.rs:51` | local |
| Clickable internal and external links | Missing | Pages are drawn as pictures with no link layer | local |
| Remember page and zoom per document | Done | `F/app/pdf_view.rs:343-358`, `:866-891` | local |
| ←/→, Space and Esc still move between items or close | Done | `F/app/pdf_view.rs:377-381` | local |
| Download, Open File, draw and free-text editing | n/a | Hidden in Eagle too | local |

### 10. 3D model viewer (Online 3D Viewer: fbx, obj, 3ds, 3mf, dae, ifc, ply, stl, glb)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Interactive 3D view (orbit, pan, zoom) | Missing | Shows a still QuickLook render if macOS can make one (`F/loader.rs:56-57`) | local |
| Fit model to window | Missing | None | local |
| Up-axis (Y/Z), flip up vector, fixed or free orbit | Missing | None | local |
| Perspective or orthographic camera | Missing | None | local |
| Measure tool | Missing | None | local |
| Model tree navigator (files, meshes, materials) and details sidebar | Missing | None | local |
| Settings: background colour, edge display, dark theme | Missing | None | local |
| Export model and create snapshot | Missing | None | local |
| Esc/←/→ still exit or move between items | Missing | n/a without a viewer | local |

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
| TIFF viewer (multi-page aware, worker decode) | Partial | Decoded at full resolution when zoomed (`Tier::Original`), QuickLook fallback; not multi-page | local |
| TGA viewer | Partial | QuickLook still | local |
| HEIC/HEIF viewer | Partial | QuickLook still | local |

### 13. Web pages (url, html, mhtml)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Live web page inside the viewer | Missing | QuickLook still at best | online/plugin |
| Back, forward, reload, copy URL, open in browser | Missing | None | online/plugin |
| Local .html/.mhtml rendered | Partial | QuickLook still of the HTML | local |

### 14. Plugin viewers

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Format plugins provide thumbnails and viewers for extra formats | Missing | No plugin system | online/plugin |

### 15. Separate preview window ("Open in New Window")

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Open one or more items in their own window (Cmd+O, middle click) | Missing | None | local |
| Same viewers in the window (image, GIF, RAW, video, font, text, PDF, 3D, web, plugin) | Missing | None | local |
| Window opacity 10%–100% | Missing | None | local |
| Always on top toggle in the window | Missing | None | local |
| Navigator, grayscale, rotate, flip in the window menu | Missing | None | local |
| Copy, copy path, copy link, reveal in Finder, open in default app | Missing | None | local |
| Copy or save the current video frame | Missing | None | local |
| Reveal in Eagle (select the item in the main window) | Missing | None | local |

### 16. Grid hover behaviours

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Hover zoom button on cards, enlarged preview after 200 ms | Done | `F/hover.rs:10`, `F/app.rs:2680-2743` | local |
| Hold Z over a card for an instant preview | Done | `F/app.rs:2702-2704`, `F/hover.rs:397` | local |
| Preview loads the original for raster formats | Done | `F/hover.rs:26`, `F/app.rs:2730-2736` | local |
| Animated GIF plays inside the hover preview | Done | `gif_view.rs` `hover_anim`, `anim.rs` `Cards::popup` | local |
| GIF and animated images play when hovered in the grid | Done | GIF and animated WebP (`card_anim`, `Cards::play`); no AVIF | local |
| Preference: always autoplay GIF/WebP/AVIF in the grid | Done | `prefs.always_play_animated` (off by default, as Eagle's `habits.alwaysPlayGIF`); no AVIF | local |
| Video hover preview: autoplays, moving the pointer scrubs, progress bar and time | Missing | Video cards have no hover behaviour (`F/hover.rs:28-31` only removes the zoom button) | local |
| Mute toggle on the video hover preview, remembered | Missing | None | local |
| Audio hover preview: plays, click to seek, progress | Missing | None | local |
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
| avif | Image viewer, animates | QL | QL still |
| jxl | Image viewer | QL (macOS 14+) | QL still |
| heic, heif, hif | Image viewer | QL | QL still |
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
| url | Live web view | QL (generic) | QL still |
| html, mhtml | Live web view | QL | QL still |

### 18. Ranked list of local Missing and Partial items

Ranked by how often a user would notice them.

1. **Viewers cover the whole window (Missing).** In Eagle, every viewer opens between the sidebar and inspector (`index.html:1007`). Falcon does this only for PDFs (`F/app.rs:3392`). This affects image, GIF, video, audio, font, text, 3D and special-format viewing.
2. **No toggle for sidebar and inspector inside the viewer (Missing)** (`toggle-all-btn`).
3. **Image resolution capped at 3072 px (Partial).** Zooming into large photos and SVGs is soft (`F/loader.rs:26`).
4. ~~No GIF frame player.~~ Fixed.
5. **Animated AVIF doesn't animate** (no decoder offline); WebP and APNG do.
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
22. **Font viewer gaps (Missing / Partial).** No tabs, colour themes, full info panel, rename from font name, activate or deactivate, manual language choice or rating keys.
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
| Also matches region and video comment text | Missing | `S:311` leaves out `comments[]` | local |
| Also matches camera (`rawMetas.camera`), text-file content (`image.text`) and font metadata | Missing | not in `S:311` | local |
| Search Scope menu: turn name, folder name, folder description, extension, tags, URL, comment and note on or off (remembered) | Missing | The caret at `A:2068` is only drawn; it has no menu | local |
| Search All: search the whole library from any view | Missing | Only the current scope is searched (`L:118` in_scope) | local |
| Exact-substring results only | Partial | Falcon adds its own typo-tolerant fallback (`S:191` fuzzy_score, `L:165`), which returns results Eagle would not. This is a deviation to keep or put behind an option. | local |
| Traditional and simplified Chinese match each other | Missing | none | local |
| Pinyin matching for folder, smart folder and tag names in sidebar and tag lists | Missing | `A:1904` subtree_matches is a plain substring test | local |
| Toolbar title reads "Search results" while searching | Done | `A:2019` | local |
| Empty-result message | Partial | Plain "No items" (`A:2449`) | local |
| Search by image from the search box (AI) | Missing | The camera icon is drawn disabled (`A:2069`) | online/plugin |

### 2. Filter bar (general)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Show or hide the filter bar | Done | `A:2045`, Command::ToggleFilter | local |
| Badge or highlight while filters are active | Done | `A:2048` outlines the funnel | local |
| Clear all filters | Done | `A:2403` Clear | local |
| Add filter (+) menu: choose and pin which filters appear in the bar | Partial | Fixed chips. Duration, BPM, Camera and Font hide when not applicable (`FU:52`); nothing can be pinned or chosen. | local |
| Lock the filter so it survives switching folders. Unlocked, a view change clears the filter and search | Partial | No lock toggle. Falcon never clears the filter or search on a view change (`A:796` go), so it always acts as if locked. | local |
| Per-option counts in each filter menu | Partial | Counts for tags, date presets, months and cameras. None for types, shapes or ratings. | local |
| A shortcut for each filter (open the colour, tag, folder... chip) | Missing | Only toggle-filter exists. Cross-ref: shortcuts agent. | local |

### 3. Individual filters

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Colour: preset swatches, any colour, accuracy slider, results sorted by colour distance | Done | `A:2237`, `S:870` ColorQuery, `L:173` distance sort | local |
| Colour: grayscale only | Done | `A:2276`, `S:922` | local |
| Colour: full picker widget | Partial | Hex text field plus 14 swatches, no picker | local |
| Tags: include tags, with Any, All or Equals logic | Done | `A:2289`, `S:362` SetFilter (keeps the OR quirk) | local |
| Tags: Untagged option | Done | `A:2294` | local |
| Tags: exclude a tag (right-click) | Missing | `SetFilter.excludes` exists (`S:362`) but has no UI | local |
| Tags: search box, Select all, and a sidebar with All, Selected and tag groups; filter by a whole group | Missing | Plain checkbox list (`A:2296`) | local |
| Folders: include folders with Any, All or Equals; Uncategorized option | Done | `A:2310` | local |
| Folders: exclude, search box, tree view | Missing | Flat list of names, no exclude | local |
| Shape: horizontal, vertical, square, panoramic horizontal, panoramic vertical | Done | `A:2331`, `S:433` | local |
| Shape: 4:3, 3:4, 16:9, 9:16 and a custom W:H ratio | Missing | Falcon has no such options; saved filters that use them are skipped (`M:690`) | local |
| Rating: 5 to 1 stars plus None, OR-ed | Done | `A:2352` | local |
| Type: pick extensions | Done | `A:2372` | local |
| Type: built-in groups (font, video, audio, YouTube, Vimeo, Bilibili), exclude by right-click, search box, counts | Partial | Groups are matched in `S:455` and `exclude_types` exists, but the UI lists only raw extensions, with no exclude, search or counts | local |
| Date Added: Today, Yesterday, last 7/30/90/365 days, custom range, months | Done | `FU:68` | local |
| Date Modified: same options | Done | `FU:68` (Modified tab). Falcon also has a Created tab, which Eagle lacks. | local |
| File size min/max in KB or MB | Missing | Engine supports it (`S:477` size); the bar has no chip. It only works through saved filters. | local |
| Dimensions: min/max width and height | Missing | Engine supports it (`S:479`); no chip | local |
| Duration min/max with s, m or h | Done | `FU:132` | local |
| BPM min/max | Done | `FU:172` | local |
| Note: has, none, or keywords | Done | `A:2393` | local |
| URL: has, none, or keywords | Done | `A:2398` | local |
| Comment (region and video comments): has, none, or keywords | Missing | No field in `S:470` Filter | local |
| Camera: pick from the cameras present | Done | `FU:201` | local |
| Font: activated or deactivated | Done | `FU:228` | local |
| Visual Match (find by image, drop or paste an image) | Missing | AI search service | online/plugin |
| Semantic search ("black cat") | Missing | AI search service | online/plugin |

### 4. Sorting

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Global sort by Date Added, Created, Modified, Title, Extension, File Size, Dimensions, Rating, Tags, with ascending/descending | Done | `S:1012` OrderBy, `S:1099` sort, `A:2125` | local |
| Global sort by Duration and Random | Partial | Offered only as a folder's own sort (`O:223`); the main sort menu lacks them (`S:1026`) | local |
| A folder or smart folder keeps its own sort, or follows Global Settings | Done | `OU:178` sort_entries, `OU:644` own_sort_menu, `O:280` finish_order | local |
| Manual sort by dragging items inside a folder | Done | `O:309` manual_sortable, `S:1122` sort_manual, OrgAction::MoveManual | local |
| Natural (numeric-aware) title order | Done | `S:1060` natural_cmp | local |
| Trash sorted by deletion time | Done | `L:169` | local |

### 5. Sidebar views and Quick Access

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| All, Uncategorized, Untagged, All Tags and Trash, with counts | Done | `A:1740` nav_rows, `L:88` nav_counts | local |
| Uncategorized also counts items whose only folder no longer exists | Partial | Falcon checks `folders.is_empty()` only (`L:123`) | local |
| Random view | Done | `L:19` Scope::Random | local |
| Random refresh button (key R) | Partial | Clicking Random again reshuffles (`A:797`); no button or key | local |
| Recently Used: items the user recently opened | Partial | Falcon shows every item by `lastModified` (`L:166`), not a list of opened items | local |
| Filter sidebar items | Partial | Filters folders only (`A:1716`), not smart folders or Quick Access; no shortcut | local |
| Go to Folder: quick-search folders, smart folders and tags, with history | Missing | none | local |
| Quick Access: add or remove a folder or smart folder | Done | `A:1842`, `OU:391`, `E:595` | local |
| Quick Access: reorder by dragging, drag a folder in to add it | Missing | Rows are click and context menu only (`A:1678`) | local |
| Quick Access: drop items on a row to add them to that folder | Missing | No dnd on Quick Access rows | local |
| Expand/collapse a folder, all folders, or the same level | Partial | Single folder by caret or double-click (`A:1863`); no all or same-level | local |
| Double-click on a sidebar row renames or collapses, per preference | Partial | Always collapses | local |

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
| Delete-folder checkbox "Move items within folders to Trash too" (on by default, can be unticked) | Done | `Library::delete_folders(keep_items)`; the dialog's box is ticked by default | local |
| Delete several selected folders at once | Done | `Library::delete_folders`, one undo step; Quick Access entries removed | local |
| Move a folder by dragging (above, below, into) | Done | `OU:255` folder_drop, `O:135` tree_drop | local |
| Confirmation dialog when dropping a folder | Partial | Moves without asking | local |
| Move Folder dialog with destination search | Done | `folders_ui` `move_dialog`; no create-folder-from-search | local |
| Move Up, Down, To Top, To Bottom | Missing | Cross-ref: menus agent | local |
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
| Folder description, edited in the inspector | Done | `scope_inspector` in app.rs keeps a `folder_edit` buffer and saves on blur through `Action::SetFolderDescription` (`edit.rs` set_folder_description), one undo step. Eagle saves a second after typing stops | local |
| Rename a folder from the inspector name field | Done | `scope_inspector`: Return or blur saves through `Action::RenameFolder`; an emptied name reverts | local |
| Folder auto-tagging: tags added to every item put in the folder, including the parent folders' tags | Done | `crates/falcon-library/src/autotag.rs` (own tags then each parent's, deduped, in Eagle's order); applied on import, paste, Add to Folder, the inspector picker and drops. `extendTags` is derived, as Eagle does, not stored | local |
| Folder settings dialog (name plus auto-tags), Cmd+Shift+R | Done | `organize_ui.rs` auto_tag_dialog; `shortcuts.rs` `edit.folder.setting`; saving appends the tags to the folder's items, as Eagle does | local |
| Show Subfolder Content (folder view includes descendants' items) | Done | `logic::folder_members`, ⌥⌘7 | local |
| Subfolder strip with covers at the top of a folder | Done | `covers.rs:65` subfolder_strip | local |
| Folder item counts | Done | `L:67` folder_counts | local |
| Folder inspector: item count, size, date imported, password state | Done | `A:2926` | local |
| Folder inspector: set, change or remove password buttons | Partial | Shows only "Set" or "None"; the actions are in the context menu | local |
| Copy Folder Link (eagle:// link) | Missing | Cross-ref: API/links agent | local |
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
| Description | Missing | Read (`SM:30`) but not editable | local |
| Several conditions, each with its own Any/All and true/false (NOT), AND-ed together | Done | `smart_draft.rs` drafts/conditions and `organize_ui::smart_conditions_ui`: every condition is edited, each with must/must-not and all/any | local |
| Editing a smart folder keeps its other conditions and rules | Done | Untouched conditions and rules are written back verbatim, edited ones keep their unknown keys, and rules the dialog can't show stay as read-only Unsupported rows until deleted (`smart_draft.rs`, test `eagle_smart_folder_survives_an_edit`) | local |
| Live "N items found" count while editing rules | Missing | none | local |
| Batch rename, recolour or delete several smart folders | Done | Multi-smart-folder menu in `folders_ui`, looping the `smart.rs` calls in one undo step | local |
| Export a smart folder | Done | Smart folder menu Export ▸ directory or `.eaglepack` (`export::plan_smart`, `PackSource::Smart`) | local |
| Rule: name, URL, note, with equals, contains, excludes, begins with, ends with, empty, not empty | Done | `smart.rs` text rules; every method is in the dialog for each text property. Begins/ends-with are anchored regexes as in Eagle | local |
| Rule: regex method | Done | `smart.rs` uses the `regex` crate, lower-casing pattern and text as Eagle does; no look-around or backreferences (documented in `docs/src/smart-folders.md`) | local |
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
| Merge tags: select several and merge them into one name | Partial | Renaming a tag to an existing name merges two at a time (`E:131`); no multi-select merge dialog | local |
| Delete a tag (with confirmation) | Done | `A:2860`, `E:169` | local |
| Star and unstar tags; Starred section | Done | `A:2856`, `E:187`, `A:2751` | local |
| Select several tags (click, Shift, Cmd, rubber band) and act on all | Missing | single-tag actions only | local |
| Tag groups: create, rename, colour, description, reorder, delete | Done | `OU:722`-`900`, `E:214`-`330` | local |
| Move a tag to a group, remove from group, drag a tag onto a group | Done | `OU:919` tag_group_menu, DragTag | local |
| Add a tag to more than one group | Missing | Only "Move to Group" (`OU:921`) | local |
| Find items with any tag in a group | Missing | none | local |
| Tag sidebar: All, Starred, Uncategorized (tags in no group), Groups | Partial | Toggle between A-Z and group view (`OU:724`); falcon lists "Ungrouped" tags in group view, but there are no separate Starred or Uncategorized pages | local |
| Sort tags by name or count, group by alphabet or groups | Partial | Grouping only; no sort by count | local |
| Tag group colour dot on chips | Done | `A:2827` tag_dot | local |
| Recent tag history (last 120 used) kept in tags.json | Done | `E:197` record_tag_use | local |
| Clear tag history | Missing | none | local |

### 9. Inspector (one item)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Preview; double-click to open | Done | `A:3034` | local |
| Colour palette swatches | Done | `A:3053` | local |
| Click a swatch to find items of similar colour | Done | `A:3066` | local |
| Copy the colour as HEX, RGB or RGBA | Partial | Right-click copies HEX only (`A:3071`) | local |
| Set a palette colour as the primary colour (`marked`) | Missing | none; `marked` is also ignored when colour-matching | local |
| Re-analyse colours | Done | `image_edit.rs:107` Refresh Palette | local |
| Name (title) editing | Done | `A:3082` | local |
| Click the extension to change it | Missing | Type is display-only (`A:3200`) | local |
| Note editing | Done | `A:3092` | local |
| URL editing, validated as Eagle does | Partial | Saved without validation (`A:3098`) | local |
| Open Link in Browser button | Missing | none | local |
| Tags: chips, remove, type a new tag | Done | `A:3107` | local |
| Tag picker with Selected, Starred, Recent, Recommended and groups, search, column sizes | Missing | Plain text field with no suggestions or autocomplete. History is recorded but never shown. | local |
| Copy, paste and clear an item's tags | Missing | none | local |
| Folders: chips, remove, Add Category | Done | `folder_chip` in app.rs; removal is the × on the chip | local |
| Click a folder chip to open that folder | Done | `folder_chip` in app.rs: the click opens the folder with the item still selected and scrolled to; the tooltip shows the folder path | local |
| Folder picker with search, Recent, Selected and create-new | Partial | Flat scrolling list (`A:3159`) | local |
| Comments list: jump to, edit, delete | Done | `comments.rs:427` | local |
| Rating stars; click the same star again to clear | Done | `A:3332` | local |
| Dimensions, size, type, date imported, created, modified | Done | `A:3196`-`3203` | local |
| Duration and BPM | Done | `media_view.rs:284` | local |
| EXIF: camera, ISO, aperture, focal length, shutter, timestamp | Done | `media_view.rs:292` | local |
| Collapse or expand inspector sections | Missing | Always expanded | local |

### 10. Batch editing (several items selected)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Preview stack and "N items selected" | Done | `A:2943` | local |
| Add a tag to all; remove a tag they share | Done | `A:2964`, Action::BatchTag | local |
| Add all to a folder | Done | `A:2988` | local |
| Show the folders they share, remove all from one | Missing | none | local |
| Edit the name, note or URL of all at once ("Multiple titles", "multiple URLs") | Missing | none | local |
| Rate all | Done | `A:3003`, Action::BatchRating | local |
| Total size | Done | `A:3007` | local |
| Batch rename with a template (%NN, $D, original name, tags, folders, index, date) and find/replace with regex | Missing | none | local |
| Confirm bulk actions on many items | Missing | none | local |
| New Folder with Selection | Missing | none | local |
| Add To Last Used Folder | Missing | none | local |
| Remove from the current folder | Missing | Only through the × on the single-item inspector chip | local |
| Pin items to the top of a folder | Missing | none | local |
| Open File Location (show the item in All or in one of its folders) | Missing | none | local |
| Rating shortcuts (keys 0-5) | Missing | No rating keys in `shortcuts.rs`. Cross-ref: shortcuts agent. | local |
| Duplicate item | Missing | Cross-ref: shell/menus agent | local |

### 11. Duplicates

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Find identical files | Done | `DV:76`, `duplicates.rs:91` content hash | local |
| Find similar-looking images, with a similarity slider | Done | `duplicates.rs:173` perceptual hash, `DV` slider | local |
| Choose the method (same or similar) before scanning | Partial | Both run in one scan | local |
| Scan all, the current list, or the selected items | Partial | Scans all only | local |
| Progress while scanning | Done | `DV` progress bar | local |
| Best copy picked automatically for similar groups (highest quality) | Partial | "Select All but First" keeps the first of each group (`DV:172`), not the best | local |
| Remove an item or a group from the results | Missing | none | local |
| Merge: keep one file and combine the others' folders, tags, link and notes into it, choosing the file name | Missing | Falcon only trashes the extra copies | local |
| Summary of files deleted and space saved | Partial | Shows group and item counts, not space saved | local |
| Move duplicates to Trash | Done | `DV:177` | local |

### 12. Trash and ratings

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Move to Trash (menu, key, drag onto Trash) | Done | `A:2537`, `A:1765` | local |
| When an item is in several folders, ask whether to remove it from this folder or trash it | Missing | Always trashes | local |
| Restore selected items | Done | `A:2531`, `E:94` | local |
| Restore All Items (with confirmation) | Partial | Restores a selection only (select all, then Restore All in the inspector, `A:3011`); no Trash menu command | local |
| Delete permanently (with confirmation) | Done | `A:2534`, `E:108` | local |
| Empty Trash (with confirmation) | Done | `A:1760`, `E:122` | local |
| Ratings stored as 1-5, key removed when cleared | Done | `A:3188`, `A:1215` | local |
| Toast "rated as N star" | Missing | Silent | local |

### 13. Comments (annotations)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Region comments on images: draw, move, resize, edit, delete | Done | `comments.rs` | local |
| Timed comments on videos | Done | `video_view.rs:472` | local |
| Filter by comments | Missing | see section 3 | local |
| Search comment text | Missing | see section 1 | local |
| Smart folder rule on comments | Missing | see section 7 | local |

### 14. Saved filters

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Save the current filter (and search) under a name | Done | `OU:958`, `E:334` | local |
| Apply, rename, remove, reorder saved filters | Done | `OU:952`-`985` | local |
| Saving keeps every active rule | Done | `SavedFilter::rule_from_filter` in model.rs writes dates (Eagle's shared `import.range`), duration (`min`/`max`/`unit`), camera and font in Eagle's key names; Date Created goes under falcon's own `btime` key | local |
| Applying restores every saved rule | Partial | `SavedFilter::to_filter` restores dates, duration, camera and font; it still skips comments, the 4:3/16:9/custom shapes and excluding Uncategorized, with a warning (organize_ui.rs saved-filter popup) | local |
| Saved filters button appears in the filter bar | Done | `OU:954` | local |

### 15. Cross-references (other agents' areas)

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Undo for organising edits with an UNDO toast | Partial | Cmd+Z works through transactions (`A:1262`); no toast button | local |
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
10. **No tag picker.** No suggestions, recent or starred tags, autocomplete or copy/paste/clear tags in the inspector.
11. **No exclude, search or tag-group options in the Tag, Folder and Type filters.** Also no type groups (video, audio, font...).
12. **Remove from current folder is missing**, and so is the "remove from folder or trash" prompt for items in several folders.
13. ~~No Show Subfolder Content toggle.~~ Fixed.
14. **Batch rename of items is missing**, and so is batch name/note/URL editing for several items (folders can be batch-renamed).
15. **Duplicates can't merge.** Folders, tags, notes and links aren't combined; there's no best-copy pick, no scope (current list or selected), and no remove-from-results.
16. **No Go to Folder quick search.**
17. ~~No multi-select of folders.~~ Fixed, for folders and smart folders.
18. **Missing folder command**: Add to Last Used Folder (New Folder Below, New Folder with Selection, Duplicate, Move Folder dialog and Move Up/Down/Top/Bottom are done).
19. **Emoji folder icons don't show.** A new folder doesn't inherit the parent's icon or colour, and there's no reset cover.
20. **No Pin to Top for items, and no Open File Location.**
21. **Tag manager gaps**: no multi-select merge, no adding a tag to several groups, no "find items with tags in group", no Uncategorized tags page, no sort by count, no clear tag history.
22. **Quick Access can't be reordered, dragged into, or used as a drop target.**
23. **Filter bar gaps**: no Filter Lock toggle (falcon always keeps the filter and search when switching views), no add or pin filters, no ratio shapes (4:3, 16:9, custom), no counts for type, shape and rating.
24. **Global sort lacks Duration and Random.**
25. **Rating gaps**: no rating shortcuts (0-5) and no rating toast.
26. **Palette gaps**: no "Set as Primary Colour", and copy is HEX only (no RGB or RGBA).
27. **No change-extension in the inspector.** URL isn't validated and there's no Open Link button.
28. **Recently Used isn't a list of opened items.** Random has no refresh key. Uncategorized ignores stale folder ids.
29. **Folder deletion**: the trash checkbox can now be unticked; there's no move confirmation, and Touch ID unlock is missing.
30. **Search extras**: no Search All, no Chinese traditional/simplified matching, no pinyin sidebar search. Falcon's own fuzzy matching is a deviation from Eagle.
31. **Minor UI gaps**: no expand/collapse all folders, no collapsible inspector sections, no bulk-action confirm, no Restore All command, no smart folder description, and the double-click preference is ignored.

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
| Screen capture of an area, a window or the full screen into the library | Missing | No screenshot code (`grep screencapture` finds only a doc comment in `collect.rs:5`) | local |
| Watched auto-import folder: files dropped into it are imported automatically | Missing | No folder watcher in the workspace | local |
| Auto-import rejects network drives and missing paths | Missing | Needs auto-import first | local |
| Auto-import skips `.tmp` files, tiny files and unsupported types, and waits until a copy has finished | Missing | Needs auto-import first | local |
| Import an Eagle Package (`.eaglepack`) from the File menu or by opening the file | Partial | Library and CLI only: `crates/falcon-library/src/pack.rs:108` `import_pack`, `src/main.rs:73-84,138-142`. No app entry point. Dropping a pack on the window imports the `.eaglepack` file as an item | local |
| Confirm the disk space an `.eaglepack` will use before importing it | Missing | — | local |
| When an `.eaglepack` holds items already in the library, ask "N repeated files, import anyway?" | Partial | CLI flag `--skip-duplicates` (`pack.rs:50`, `main.rs:82`); no prompt | local |
| Merge another Eagle library into the current one (folders and items), refusing the library already open | Missing | — | local |
| Import an Inboard backup folder (`InboardBackup.json`), with its folders recreated | Missing | — | local |
| Import a Pixave library | Missing | — | local |
| Import from Billfish | — (not counted) | — | — |
| Import from Pinterest | Missing | — | online |
| Import from Huaban | Missing | — | online |
| Import an ArtStation artist's portfolio by URL into a folder, all at once or picked by hand | Missing | — | online |
| Add files by opening them with the app from Finder (Eagle registers for about 36 image, video and design types, folders and packages) | Partial | CLI arguments that are existing paths are imported (`crates/falcon-app/src/main.rs:38`). The bundle registers no document types (`scripts/bundle.sh`), and Finder "Open With" events are not handled | local |
| Add items from another app through the HTTP API (`addFromPath`, `addFromPaths`, `addFromURL`, `addFromURLs`, v2 `item/add`) | Done | `crates/falcon-server/src/lib.rs:369-372,386`; data URLs and http(s) downloads in `add_url` (`lib.rs:753-779`) | local, or online for URLs |
| Add an item from base64 data (v2 `item/add` with `base64`) | Done | `crates/falcon-server/src/v2.rs:303-305` | local |
| Import into a folder chosen in the collect window | Done | `app.rs:768-776` | local |

### 2. Import options and behaviour

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Copy the original into `images/<ID>.info`, keep the source mtime, make a thumbnail and palette | Done | `crates/falcon-library/src/import.rs:79-147,173-207` | local |
| Imports run in the background with a progress display | Done | `app.rs:888-938` (background thread and progress bar) | local |
| Upload queue panel listing failed imports with reasons (download failed, timeout, cannot decode and so on), plus Retry All, Copy to Clipboard and Clear All | Done | `import::Failure` / `FailKind` per file; `import_ui.rs` `error_panel` with Retry All, Copy to Clipboard (the files), Clear All; opened from a toast. Local-file reasons only | local |
| "Import Successful, N image(s) imported" notification | Partial | In-window flash only (`app.rs:924-925`), no system notification | local |
| Duplicate check on import (Eagle's metadata key), with a dialog offering Import, Don't Import, Keep both, Use existing file and Apply to all | Done | `import_check.rs` (same size and extension, then a BLAKE3 hash of the whole file), `crates/falcon-app/src/import_dupes.rs`, `app/dupes_ui.rs`: Eagle's two choices (use existing, keep both), Import, close = don't import, apply to all. One thumbnail shown, not two | local |
| Setting: notify me when importing duplicates | Done | `Prefs::notify_duplicates`, on by default; off imports duplicates silently as Eagle does | local |
| Folder auto-tagging: tags set on a folder are added to every item imported or pasted into it | Done | `ImportOptions::folder_auto_tags`, set by `Prefs::import_options`; applied in `commit_import` | local |
| Screenshot auto-tag ("Screenshot" tag) | Missing | No screenshots | local |
| Add IPTC and XMP keywords as tags (setting) | Done | `prefs.rs:24,327`; `import.rs:121-123` | local |
| Font style tag (Regular, Bold, Italic) added on import (setting) | Done | `Prefs::font_style_tags` in the Preview pane, on by default | local |
| Auto-select the new items after import (setting) | Done | `Prefs::auto_select`, on by default; selects the new items shown in the view and scrolls to a single one | local |
| Unsupported formats are refused with an "unsupported format" dialog | Done | `import::is_recognized`: like Eagle, only extensionless files and unfinished browser downloads (`.crdownload`, `.part`, `.download`) are refused, into the error list; every other type imports | local |
| Empty files are refused (except `.txt`) | Done | `import.rs:88-90` | local |
| Hidden files are skipped during folder walks | Done | `app.rs:149-151,872-876` | local |
| Not-enough-disk-space check before copying | Done | Eagle has no pre-copy check on import either; ENOSPC is reported per file as "Not enough disk space" in the error list | local |
| Filename sanitising and length limit | Done | `import.rs:411-430` | local |
| Cut mode: files staged in app data (pasted or downloaded) are moved, not copied | Done | Pasted temp files are deleted after import (`app.rs:893-897`) | local |
| Choose where a screenshot or extension item is saved (collect prompt) | Missing | The collect window exists, but nothing triggers it, since there is no extension or capture | local |

### 3. Collect window

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| A small floating window that stays on top and takes drops and pastes | Done | `crates/falcon-app/src/collect.rs:83-95` (always on top, borderless) | local |
| Choose the destination folder, with search | Done | `collect.rs:28-54` (flattened tree with search) | local |
| Send to several folders at once | Missing | One folder only (`collect.rs:50`) | local |
| Recent folders list | Missing | — | local |
| Create a new folder from the collect window | Missing | — | local |
| Set name, description and tags before collecting (tag picker with recent, starred and recommended tags) | Missing | Falcon's collect window has no metadata fields | local |
| Toggle the collect prompt over the API (`/api/preferences/collect/on`, `/off`) | Missing | Route not registered (`lib.rs:355-411`) | local |
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
| "Export Successful, N item(s) exported" notification | Partial | In-window message plus a Finder reveal; no system notification | local |
| Drag items out to Finder or other apps | Done | `app.rs:489,1451,3714` (`native_drag`, `drag_image`, `drag::start_drag`) | local |
| Copy the selected files to the clipboard (⌘C) | Done | `app.rs:941-953` | local |
| Copy File Path (⌥⌘C) | Done | All selected items, one per line (`export_ui.rs`) | local |
| Copy folder path (⌥⇧⌘C) | Done | The selected items' `<id>.info` folders, as Eagle | local |
| Copy Name | Done | `export_ui.rs` `item_copy_menu` | local |
| Copy Thumbnail (as an image) | Done | One item: image on the clipboard; several: the thumbnail files, as Eagle | local |
| Copy Base64 | Done | One item: data URL of the original for jpg/png/webp/gif, else of the thumbnail | local |
| Copy Eagle Link (`eagle://item/<id>`) | Missing | — | local |
| Copy Link (the item's source URL) | Done | `edit.copy.sourceurl`; Eagle's own Copy Link copies its local API link instead | local |
| Copy the image to the clipboard as pixels plus file reference (the AppleScript helper) | Partial | Falcon copies a file list only (`app.rs:948-949`) | local |
| macOS Share sheet for items | Missing | — | local |
| Add selected items to another library (from the recent list, or choose a path) | Missing | — | local |
| Convert items to WebP to save space | Missing | — | local |
| Save the current video frame as an item or copy it | Missing | Not in the video viewer's actions; see the viewers audit | local |

### 5. Library management

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Create a library | Done | `app.rs:779-788`; `crates/falcon-library/src/library.rs:98-122` | local |
| Create-library dialog with a name field, name validation and folder choice | Partial | Falcon uses a plain save dialog defaulting to `Untitled.library`. It has no name validation, though it adds `.library` | local |
| Warn when creating a library in Desktop, Documents or Downloads | Missing | — | local |
| Refuse a path that already holds a library | Done | `library.rs:101-106` refuses any existing path | local |
| Refuse libraries inside the app's install folder | Missing | — | local |
| Open an existing library | Done | `app.rs:790-794,594-618` | local |
| Reject a folder that is not a library | Done | `library.rs:53-55` (`NotALibrary`), shown in the empty state (`app.rs:3537-3539`) | local |
| Switch library (⌘L) through a switcher listing libraries | Done | `app.rs:998-1001,1607-1650` | local |
| Recent libraries history | Done | `crates/falcon-app/src/workspaces.rs:119-129`, `app.rs:1627-1635` (10 entries, `app.rs:1436`) | local |
| Clear library history | Missing | — | local |
| Remove one library from the list | Missing | — | local |
| Delete a library folder from the list menu | Missing | — | local |
| Set or remove a library icon (`icon.png`) | Missing | The API serves an `icon.png` (`v2.rs:920-925`), but the app cannot set one; the sidebar always shows a generic icon (`app.rs:1597-1599`) | local |
| Warn before switching while tasks run (Force Switch), or block while an AI action runs | Missing | Imports keep running against their own shared library handle; there is no prompt | local |
| Several libraries open at once | Done (beyond Eagle) | `workspaces.rs` (several workspaces), `app.rs:621-689` | local |
| Show the library in Finder | Done | `app.rs:1643-1645` | local |
| Missing-library screen with Try Again, Select another path and Create a new library | Partial | The empty state shows the error text and "Choose Library…" (`app.rs:3533-3545`); restore silently drops missing libraries (`workspaces.rs:103`). No retry and no relocate flow | local |
| Relocate a library that moved | Partial | Only by reopening it through Open Library | local |
| Rename a library | — (not counted) | — | — |
| Refuse a library made by a newer Eagle and prompt to update | Missing | `Library::open` never checks `applicationVersion` (`library.rs:51-93`) | local |
| Automatic `metadata.json` backups (on change and every 30 minutes, up to 100 kept) | Done | `crates/falcon-app/src/backups.rs` schedules a backup 1 s after a metadata write and every 30 minutes, run off the UI thread by `run_backups` in app.rs; `flush_and_backup` takes a waiting one when a library closes or the app saves state. Written by `backup::backup_library` | local |
| Restore a damaged `metadata.json` from the newest good backup on open | Done | `App::open` and `follow_api_switch` use `Library::open_or_restore`; `restored_note` names the backup used in the toast. The HTTP API's own `library/switch` still uses `Library::open` | local |
| Keep a per-library backup copy in app data (`eagle-backup/<hash>.json`) | Missing | — | local |
| Empty cache and reload the library (forced rebuild) | Missing | Falcon keeps no library cache, but has no "reload from disk" either | local |
| Repair tool "Clear Invalid Content" (removes empty or broken item folders, then reloads) | Partial | Unreadable items are skipped and reported on open (`library.rs:63-73`, `app.rs:607-610`), but cannot be cleaned up | local |
| Refresh thumbnails for the selection | Done | `crates/falcon-app/src/image_edit.rs:104-106,151-171` | local |
| Refresh the palette for the selection | Done | `image_edit.rs:107-109` | local |
| Detect external changes to `metadata.json` and `mtime.json` (sync tools) and reload | Missing | No file watching; changes made by other apps appear only after reopening | local |
| Cloud-sync conflict detection (disk copy newer than memory, so warn and reload) | Missing | — | local |
| iCloud placeholder handling (`.icloud` names, files not yet downloaded) | Done | `crates/falcon-app/src/icloud.rs` (dataless detection, background download, wait screen in `app/open_ui.rs:128-180`) | local |
| "File not found, sync may be in progress" hint for missing originals | Partial | iCloud files are handled; other missing originals get no specific hint | local |
| Library storage health warnings (read-only path, slow disk, NTFS on macOS, permission errors) | Missing | — | local |
| Library loading progress screen | Missing | `Library::open` runs on the UI thread with no progress (`app.rs:601`) | local |
| Library stats (item count, size, trash count) | Partial | Sidebar counts (`app.rs:1744-1745`) and folder inspector items and size (`app.rs:2925-2928`); CLI `info` prints items, trash, folders and tags (`src/main.rs:112-121`). No library-wide stats in the app | local |
| Welcome or onboarding flow (theme, extension install, create library) | Missing | Plain empty state (`app.rs:3531-3546`) | local |
| Password lock for the whole app: password, hint, Lock Now (⌥⌘L), Touch ID unlock | Partial | `crates/falcon-app/src/applock.rs`: password with hint, salted BLAKE3 digest in app state, lock screen at launch, ⌥⌘L. No Touch ID | local |

### 6. Preferences (every pane and setting)

Eagle panes (`app/js/preferences.js:659-788`): General, Sidebar, Controls, Preview (habits), Screenshot, Shortcuts, Notification, Password, Auto-Import, AI Search, AI Models, Eagle MCP and Developer. The Proxy pane is in the i18n file but not listed in this build. Falcon panes: General, Preview, Shortcuts and Developer.

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Preferences window with Save, Apply and Cancel | Done | `prefs.rs:250-263` | local |
| Search box across preferences | Done | `prefs.rs` `matches` over every pane's blocks | local |
| Remember the last open pane | Done | `Saved.prefs_pane` | local |
| **General:** theme (Auto, Light, Light Grey, Gray, Dark, Blue, Purple) | Done | `crates/falcon-app/src/theme.rs:73-83`, `prefs.rs:312-321` (live preview, reverted on Cancel) | local |
| General: language | Missing | English only (no i18n code) | local |
| General: UI zoom level (50 to 150%) | Done | `Prefs::ui_zoom` in Eagle's steps, previewed live; egui's own keyboard zoom is disabled so ⌘+/⌘− only zoom the grid | local |
| General: transparency (vibrancy) effects | Missing | — | local |
| General: show counts in the sidebar | Done | `Prefs::sidebar_counts` | local |
| General: menu bar icon | Missing | No status item | local |
| General: keep the Dock icon after the window closes | Missing | — | local |
| General: launch at login | Missing | — | local |
| General: GPU acceleration | Missing | — | local |
| General: auto-select items after import | Done | `Prefs::auto_select` | local |
| General: notify on duplicate import | Done | `Prefs::notify_duplicates` | local |
| General: ask where to save items from the extension | Missing | — | local |
| General: @2x screenshots | Missing | — | local |
| General: IPTC keywords as tags | Done | `prefs.rs:326-328` | local |
| General (falcon only): show the sidebar and inspector | Done (beyond Eagle) | `prefs.rs:322-325` | local |
| **Sidebar:** double-clicking a sidebar item renames it or expands and collapses it | Done | `Prefs::sidebar_double_click` (default: expand/collapse) | local |
| Sidebar: show or hide Uncategorized, Untagged, Recently Used, Random, Community, Quick Access, Smart Folders and Folders | Done | `SidebarSections`, all on by default; no Community entry | local |
| **Controls:** mouse wheel in the viewer scrolls, pages to previous or next, or zooms | Done | `prefs.wheel` | local |
| Controls: hover zoom button on cards | Done | `prefs.rs:331-337` | local |
| Controls: double-clicking an item opens it in Eagle or in the default app | Done | `Prefs::double_click` | local |
| Controls: middle button does nothing, opens a new window or runs a plugin | Missing | — | local |
| Controls: spacebar previews, opens Quick Look or scrolls the list | Done | `Prefs::space` | local |
| **Preview:** zoom rendering smooth or pixelated | Done | `prefs.pixelated` | local |
| Preview: remember the last zoom and position | Done | `prefs.remember_zoom` | local |
| Preview: default image size, smart zoom or original | Done | `prefs.open_actual_size` | local |
| Preview: rotation applies to the file or to the preview only | Missing | Rotation always writes the file (`image_edit.rs:113-118`); no preview-only mode | local |
| Preview: transparency grid on or off | Done | `prefs.transparency_grid` | local |
| Preview: video vertical scroll controls progress or volume | Done | `Prefs::video_scroll` | local |
| Preview: video hover preview | Missing | Falcon has no video hover-play; `hover.rs` is the still-image preview | local |
| Preview: stretch video to fit the window | Done | `Prefs::video_fill` | local |
| Preview: video autoplay | Done | `Prefs::video_autoplay` | local |
| Preview: remember the playback position | Done | `Prefs::video_resume`, saved per item across launches | local |
| Preview: loop clips shorter than 30 s | Done | `Prefs::video_loop_short` | local |
| Preview: autoplay GIF, WebP and AVIF in the grid | Done | `prefs.always_play_animated`; no AVIF | local |
| Preview: advanced GIF viewer | Done | `prefs.gif_player` | local |
| Preview: automatic font style tags | Done | `Prefs::font_style_tags` | local |
| **Screenshot:** format PNG, JPG or WebP | Missing | — | local |
| Screenshot: quality | Missing | — | local |
| Screenshot: enable the global shortcut, plus area, window and full-screen shortcuts | Missing | — | local |
| Screenshot: ask where to save | Missing | — | local |
| Screenshot: notify when taken | Missing | — | local |
| Screenshot: auto-tag "Screenshot" | Missing | — | local |
| Screenshot: also copy to the clipboard | Missing | — | local |
| Screenshot: @2x on Retina | Missing | — | local |
| **Shortcuts:** rebind any command, search the list, Restore Defaults | Partial | Recorder, search field and Restore Defaults (`shortcuts.rs`); only falcon's 60-odd commands | local |
| Shortcuts: plugin shortcuts | Missing | No plugins | local |
| Global shortcuts (show Eagle, quick search from anywhere) | Missing | — | local |
| **Notification:** sound effects master switch, plus per event (delete item, delete folder, screenshot, extension) | Missing | — | local |
| Notification: pop-up master switch, plus per event (screenshot, extension, duplicates, auto-import) | Partial | "Show notices" master switch gating the duplicate dialog and the import-done note; screenshot, extension and auto-import don't exist | local |
| Notification Center panel (history) | Missing | — | local |
| **Password:** enable the app password lock, change the password, Lock Now, Touch ID | Partial | Set, change, remove, hint, Lock Now ⌥⌘L (`applock.rs`); no Touch ID | local |
| **Auto-Import:** enable, choose folder, open folder | Missing | — | local |
| **AI Search** pane (install, enable or disable the local semantic-search plugin) | Missing | — | online/plugin |
| **AI Models** pane (shared API keys for OpenAI, Anthropic, Gemini and so on) | Missing | — | online/plugin |
| **Eagle MCP** pane (install or open the MCP server plugin) | Missing | — | online/plugin |
| **Developer:** show, copy and regenerate the API token | Missing | Falcon has no token auth; the Host check is the only gate (`lib.rs:190-194`) | local |
| Developer (falcon only): API on or off, and first port | Done (beyond Eagle) | `prefs.rs:339-358` | local |
| Developer: plugin developer tools | Missing | — | local |
| **Proxy** (server address and port, used for downloads and web search) | Missing | — | online |
| Color space (sRGB, Display P3, unmanaged) | Missing | — | local |

### 7. HTTP API

Falcon serves the API on 127.0.0.1 from port 41596 upward (never 41595), while a library is open in the app or through `falcon serve` (`lib.rs:69`, `prefs.rs:12-15`, `src/main.rs:85-92`). Routes are registered in `crates/falcon-server/src/lib.rs:355-411`.

#### 7.1 v1

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| `GET /` and `/api/application/info` | Partial | `lib.rs:426-431`: version, build and platform only. No `preferences` or `showCollectModal` (a deliberate choice) | local |
| `GET /api/library/info` | Done | `lib.rs:438` | local |
| `GET /api/library/history` | Partial | Lists libraries this server has served (`lib.rs:448`, header comment `lib.rs:14`), not the app's recent list | local |
| `POST /api/library/switch` | Done | `lib.rs:450-461`; the app follows it (`app.rs:692-722`) | local |
| `GET /api/library/icon` | Done | `lib.rs:462`, `v2.rs:920` | local |
| `GET /api/folder/list` | Done | `lib.rs:465` | local |
| `GET /api/folder/listRecent` | Done | `lib.rs:466` | local |
| `POST /api/folder/create`, `rename`, `update` | Done | `lib.rs:469-501` | local |
| `POST /api/folder/unlock` | Missing | Not registered | local |
| `GET /api/tag/all`, `list`, `listRecent`, `groups` | Done | `lib.rs:502-522` | local |
| `POST /api/item/addFromPath`, `addFromPaths` | Done | `lib.rs:369-370` (imports finish before the reply) | local |
| `POST /api/item/addFromURL`, `addFromURLs` | Done | `lib.rs:668-680,753-779` | online for http URLs |
| `POST /api/item/addBookmark` | Missing | Not registered | online |
| `POST /api/item/batchSave` | Missing | Not registered | online |
| `POST /api/item/update` | Done | `lib.rs:373` | local |
| `GET /api/item/info`, `thumbnail`, `list` | Done | `lib.rs:374-377,782-800` | local |
| `POST /api/item/moveToTrash` | Done | `lib.rs:376` | local |
| `POST /api/item/setCustomThumbnail` | Missing | The library supports it (`set_custom_thumbnail`, used at `image_edit.rs:142`), but the route is not registered | local |
| `POST /api/item/refreshPalette`, `refreshThumbnail` | Missing | The library supports both (`image_edit.rs:160`), but the routes are not registered | local |
| `GET /api/preferences/collect/on`, `/off` | Missing | Not registered | local |
| `/item?id=`, `/folder?id=` and `/smart-folder?id=` redirects to `eagle://` links | Missing | Not registered | local |
| `POST /api/script/inject` (runs arbitrary JavaScript) | Missing (on purpose) | Deliberately left out; a security hole | local |
| `GET /api/check` (obfuscated remote-script loader) | Missing (on purpose) | Not library functionality | online |
| Token auth (`?token=` matching the Developer token) | Missing | Host-header check only (`lib.rs:190-194`) | local |
| Listen on all interfaces | Different (on purpose) | 127.0.0.1 only (`lib.rs:9,69`) | local |

#### 7.2 v2

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| `app/info` | Done | `lib.rs:432` | local |
| `library/info`, `history`, `switch`, `icon` | Done (history is per server, as in v1) | `lib.rs:379-382,439-463` | local |
| `item/get`, `query`, `countAll`, `add`, `update` | Done | `lib.rs:383-387,540` | local |
| `item/add` with `bookmarkURL` | Missing | `v2.rs:301-302` | online |
| `item/getComments`, `addComment`, `updateComment`, `removeComment` | Done | `lib.rs:388-391,541-544` | local |
| `item/setCustomThumbnail`, `item/refreshThumbnail` | Missing | Not registered | local |
| `folder/get`, `create`, `update` | Done | `lib.rs:392-394,545-546` | local |
| `smartFolder/get`, `create`, `update`, `remove`, `getItems`, `getRules` | Done | `lib.rs:395-400,534-539` | local |
| `tag/get`, `getRecentTags`, `getStarredTags`, `update`, `merge` | Done | `lib.rs:401-405,523-527` | local |
| `tagGroup/get`, `create`, `update`, `remove`, `addTags`, `removeTags` | Done | `lib.rs:406-411,528-533` | local |
| `aiSearch/*` (isInstalled, isReady, isStarting, isSyncing, getSyncStatus, checkServiceHealth, searchByText, searchByBase64, searchByItemId) | Missing | — | online/plugin |
| Interactive API playground page | Missing | — | local |

### 8. Integrations

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| `eagle://` URL scheme registered with macOS | Missing | The bundle Info.plist has no URL types (`scripts/bundle.sh:12-30`) | local |
| `eagle://item/<id>` opens the item | Partial | Accepted as a **launch argument** only (`crates/falcon-app/src/main.rs:39`); not a handler for links clicked elsewhere | local |
| `eagle://folder/<id>`, `eagle://smart-folder/<id>`, `eagle://openFolder/<id>` | Missing | — | local |
| Open `.eaglepack` files by double-clicking them | Missing | — | local |
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
| Help: Error Logs and Export Debug Report | Missing | — | local |
| Help: help centre, changelogs, roadmap, feedback, tutorial, tips | Missing | Cross-ref menus audit | online |
| Menu bar (tray) icon with capture actions, Show Eagle and Quit | Missing | — | local |
| Scripted launch options (`--search`, `--color`, `--tags`, `--select`, item ids, files) | Done (beyond Eagle) | `crates/falcon-app/src/main.rs:20-41` | local |
| Command-line tool (`import`, `new`, `folder`, `info`, `backup`, `export`, `import-pack`, `serve`) | Done (beyond Eagle) | `src/main.rs:17-93` | local |

### 9. Localisation

| Feature | Falcon | Falcon evidence or gap | Local? |
| --- | --- | --- | --- |
| Eight UI languages: English, Simplified Chinese, Traditional Chinese, Japanese, Korean, Spanish, German and Russian (picked automatically from the system language, or set in Preferences) | `i18n/index.js` `supportLanguage`; `i18n/{en,zh_CN,zh_TW,ja_JP,ko_KR,es_ES,de_DE,ru_RU}.js`; collect window `locales/*.json`; PH `general.language` | Missing | All strings are hard-coded English; no i18n code (`grep language\ | locale` finds nothing in `crates/falcon-app/src`) | local |
| Chinese pinyin search support (jieba dictionaries) | Missing | Model round-trips `pinyin` only | local |

---

### Counts

Rows marked "not counted" are features Eagle 4.0.0 does not have.

- **Done:** 111, including 5 where falcon goes beyond Eagle and 1 with a small difference.
- **Partial:** 19.
- **Missing:** 108, including 3 left out on purpose (`script/inject`, `/api/check`, licensing).
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
8. **Watched auto-import folder is missing**, along with its preference pane.
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
19. **App password lock** has no Touch ID.
20. **Missing API routes**: `folder/unlock`, `item/setCustomThumbnail`, `item/refreshThumbnail`, `item/refreshPalette` (v1 and v2), `preferences/collect/on|off`, and the `/item`, `/folder` and `/smart-folder` redirects. There is no token auth, and `library/history` is per server rather than the app's recent list.
21. **Hard-coded behaviours need preference switches**: font style tag, video autoplay, loop short clips, hover video preview, saved playback position, sidebar counts.
22. **Missing Controls and Preview settings**: middle button, and the image settings not yet wired (see §6).
23. **Sidebar pane is missing**: show or hide sections, and double-click to rename or collapse.
24. **Notifications and sounds are missing**: system pop-ups and sound effects per event, and the Notification Center.
25. **General settings are missing**: language, UI zoom, vibrancy, launch at login, Dock icon, menu bar icon, GPU, auto-select after import.
26. **Localisation**: English only, against Eagle's eight languages.
27. **Library safety checks are missing**: newer-version refusal, warnings for Desktop or Documents paths, read-only paths and slow disks, disk-space checks, and confirmation for large folders or deep trees.
28. **"Empty cache and reload" and "Clear Invalid Content" are missing.** Damaged items are skipped and reported, but cannot be cleaned.
29. **Smaller gaps**: library loading progress, welcome flow, Preferences search, remembering the last pane, the Share sheet, WebP conversion, saving the current video frame, the menu bar icon, Inboard and Pixave importers, the API playground, debug report and error logs, the plugin system and plugin development, and colour-space selection.
