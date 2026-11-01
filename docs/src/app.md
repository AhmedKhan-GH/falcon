# The desktop app

`falcon-app` is falcon's version of Eagle's main window, built with egui. Its look comes from Eagle itself:

- **Colours:** dark is sampled from Eagle's window and its dark-theme stylesheet (`style/base/_color.scss`). For example, the content area is `#17191d`, the panels `#1f2023` and the accent `#3297ff`. Light, lightgray, gray, blue and purple are worked out from the stylesheets, and Auto follows the macOS appearance (see Themes below).
- **Fonts and icons:** SF Pro for text, plus Phosphor Light icons, which are MIT-licensed and close to Eagle's line icons.
- **Measurements:** taken from Eagle at 1280 × 800: 240 pt panels, a 48 pt toolbar, 27 pt sidebar rows and 139 pt cards.

## Sandbox

While falcon is in development, the app and the `falcon` CLI only open libraries inside `~/Falcon Sandbox`. Set `FALCON_SANDBOX` to use another folder. Opening, creating or switching to a library anywhere else (including over the HTTP API's `library/switch`) fails with a message saying so, and the Open and Create dialogs start in the sandbox. To work on a real library, copy it into the sandbox first.

The app opens libraries with `Library::open_or_restore` (see [Backups](library-format.md#backups)): when a library's `metadata.json` can't be read or parsed, the newest usable backup in its `backup/` folder replaces it before the library loads, and a message names the backup used and says the damaged file was kept in `backup/`. This covers the switcher, Open Library, libraries reopened on launch, dropped libraries and the library the app reopens after an API `library/switch`. The HTTP API's own `library/switch` still uses `Library::open`, so a damaged library fails there.

## What it does

| Area | Features |
|---|---|
| Window | No title bar: the traffic lights sit over the sidebar, and empty bar space drags the window |
| Library menu | The open libraries (each with a close button), then recent ones, then Open, New, Show in Finder, the collect window and Preferences. Several libraries can be open at once (see Several libraries). The open libraries, layout and sort reopen on launch |
| Preferences | ⌘, or the library menu (see below) |
| Sidebar | Eagle's fixed entries, Quick Access, a nested smart folder tree, and a folder tree with guide lines and direct-item counts. Folders and smart folders show their icon in their colour, and locked folders a lock. You can filter it, create, rename and delete folders, drag folders and smart folders to reorder or nest them, and drop items onto folders or the Trash (dropping on a folder moves the items out of the open folder, and ⌥ adds them instead; see Organising below) |
| Toolbar | The panels button (see Viewer layout), back and forward, zoom slider, layout and sort menu, filter bar toggle, search with Eagle's syntax, plus typo tolerance (see Search and filters) |
| Filter bar | Colour (presets, hex value, accuracy, grayscale), tags, folders, shape, rating, type, notes, URL, date, duration, BPM, camera and font (see below), and saved filters |
| Grid | Four layouts. **Waterfall** (the default): equal columns, each card keeping its image's shape. **Justified**: rows of equal height. **Grid**: square cards. **List**: rows showing type, size, dimensions and date. Cards have type badges. Supports multi-select, drag (inside the app, or out to Finder and other apps), a context menu and a hover preview (see below) |
| Inspector | **One item:** preview, palette (click a colour to search for it), editable title, notes and URL, tags, folders (as in Eagle, clicking a folder chip opens that folder with the item still selected and scrolled into view, and the chip's × removes the item from the folder, one ⌘Z step; the tooltip gives the folder's full path), comments (edit and delete; click a region comment to show it, or a timed comment to play the video from its moment), rating and properties (with duration, BPM as stored (`119.985`, as Eagle's inspector shows it), camera facts from `rawMetas`, and font family, style, PostScript name, version and glyph count when the item has them). **Several items:** a preview stack, shared tags, add a tag to all, add all to a folder, a shared rating, trash all. **Nothing selected, inside a folder:** the folder's name and description, item count, size, date imported and password state. The name saves when the field loses focus or on Return (an emptied name goes back to the saved one), the description (several lines) when it loses focus; each is one ⌘Z step and changes only that key in `metadata.json`. Eagle saves both a second after typing stops; falcon waits for the field to lose focus, and also saves text left in the fields when the folder or the selection changes. Other scopes show their fixed name |
| Viewer | Opens between the sidebar and the inspector, with a bar holding the panels button, back, an "n / total" counter, the name, previous and next (see Viewer layout). Zoom around the pointer, pan, fit (0 or ⌘9), actual size (1 or ⌘0), + and − (or ⌘= and ⌘−), animated GIFs, full-size QuickLook renders, and a cannot-preview page with an Open With button. Right-click opens the item menu; double-click leaves. Video plays with a control bar and timed comments, PDFs open in a scrolling reader with find and text selection, audio plays over its waveform and fonts show a live specimen; images take region comments, rotate, flip and crop (see below) |
| Card menu | Besides Open, Show in Finder and the folder and Trash entries: Set as Folder Cover (inside a folder), Set Custom Thumbnail… and Reset Thumbnail, Refresh Thumbnail, Refresh Palette, Rotate Left and Right, Flip Horizontal and Vertical, and Crop… (opens the viewer with the crop tool) |
| Folder view | Cards for the folder's subfolders above its items, each with its cover, name and counts. Click one to open it |
| All Tags | A tag manager with a filter, starred tags, rename, merge and delete, grouped A-Z or by tag group |
| Duplicates | Finds identical files and similar pictures, grouped, with Select All but First and Move to Trash (see below) |
| Import | Drag and drop, ⌘V (files or images), files passed to the app, and the collect window (see below). Where a drop lands matters, as in Eagle: on the grid, files and the contents of folders go flat into the open folder; on a sidebar folder, files go into that folder; on the rest of the sidebar, each folder is imported with its subfolder tree recreated as Eagle folders at the top level, and the new folder opens (see [Importing folders](import.md#importing-folders)). The library menu also has Import Files… and Import Folders… (several folders, each with its structure kept), and the empty All and folder pages have an Import Folders… button. A folder with 2,000 files or more asks first, and one 15 levels deep or with 100,000 files or more is refused, as in Eagle. Pixave libraries are skipped. Files that fail to import are listed with a reason in a panel opened from the notice under the toolbar, with Retry All, Copy to Clipboard and Clear All (see [Failed imports](import.md#failed-imports)). Imports run in the background with a progress bar. PDF, video, PSD, AI, HEIC and RAW go through QuickLook. A dropped or pasted `*.library` folder is opened as a library (through the sandbox check, like the switcher), never imported; if several are dropped at once, only the first opens and a message says how many were left closed. As in Eagle, this applies to the dropped paths themselves, not to `.library` folders found inside a dropped folder |
| Smart folders | Create and edit them with every condition (each matched by any or all of its rules, and inverted or not) and every rule property, inside another smart folder, with an icon and colour. Rules falcon can't edit are kept |
| Trash | Restore, Delete Permanently and Empty Trash, all with confirmation |
| HTTP API | Runs while the app is open, from port 41596 up |

## Date, duration, BPM, camera and font filters

Five chips follow the existing ones in the filter bar. They set the `imported`, `modified`, `created`, `duration`, `bpm`, `cameras` and `font` rules of the library's `Filter` (see [Search and filters](search.md)), so the grid applies them like any other rule. The code is in `crates/falcon-app/src/app/filters_ui.rs`, with the parsing and labels in `logic.rs` (BPM's are in `filters_ui.rs`).

What Eagle does, from `js/directives/filter-item-import.html`, `filter-item-mtime.html`, `filter-item-duration.html`, `filter-item-bpm.html` and `.js`, `filter-item-camera.html` and `filter-item-fonts.html`:

| Question | Eagle | Seen or inferred | falcon |
|---|---|---|---|
| Which chips show | Every filter item is hidden unless it is active or pinned (`hide-filter` when `!isEnabled && !pinned[type]`); pinning is a user choice | Seen | Date always shows. Duration, BPM, Camera and Font show only when the library has an item they can match (a `duration`, a non-zero `bpm`, a camera, a font) or they are active. There is no pinning |
| Date | Two separate items, Date Imported and Date Modified. Each menu has the six presets (Today, Yesterday, Last 7, 30, 90 and 365 days) with counts, then every month that has items (`YYYY-MM`, newest first, with counts), then a Range checkbox that opens a calendar picker | Seen | One Date chip with a tab for Imported, Modified and Created (Created is falcon's own; Eagle only has it in smart folders). Each tab has the presets with counts, a range typed as two days (`2026-01-01` – `2026-12-31`, swapped if reversed, both needed), and the month list with counts. There is no calendar widget |
| Duration | Min and max text fields and a unit (seconds, minutes, hours) | Seen | Same, and a field also accepts a time such as `1:30`. The placeholders show the library's shortest and longest duration. The chip reads `0:30 – 2:00`, `≥ 0:30` or `≤ 2:00` |
| BPM | A 160 px menu with Min and Max text fields and no unit. The chip reads `90≤BPM≤128`, `BPM≥90` or `BPM≤128` from the typed text, and only bounds above 0 count for the label. The search reads each bound with `parseInt`. The `find.filter.bpm` shortcut has no default | Seen | Same two fields, read the way `parseInt` reads them (`120.5` and `120 bpm` give 120). The placeholders show the library's lowest and highest tempo. The chip reads `BPM: 90 – 128`, `BPM: ≥ 90` or `BPM: ≤ 128`, like Duration. Applying a saved filter fills the fields from its bounds. No shortcut |
| Camera | Every distinct `rawMetas.camera` value with its count; several can be ticked; "no results" when there are none | Seen | Same, counting items not in the Trash |
| Font | Two exclusive options, Activated and Deactivated, each with its count and shown only when its count is above 0 | Seen | Both options always listed with counts. "Activated" means the font's `<postScriptName>_.<ext>` key is among the files in `~/Library/Fonts/EagleApp/` (`installed_font_keys`), read once per session. On a Mac where Eagle never activated a font that folder is missing or empty, so every font counts as deactivated. falcon can't activate fonts itself |
| Clearing | A clear button on each item, and right-click | Seen | A Clear button inside each menu, and the bar's Clear for everything |

## Viewer layout

Every viewer (image, GIF, RAW, SVG, video, audio, font, PDF, the cannot-preview page and the iCloud wait screen) opens in the middle of the window, in place of the toolbar, filter bar and grid, with the sidebar and the inspector still beside it, as Eagle's detail mode does. The inspector shows the viewed item: the selection follows it as you step through, so closing the viewer leaves the grid on that item. The viewer draws only inside its own rect, and its scroll, pinch, clicks and menus act only there. The span is `logic::viewer_span`; the bar is `media_view::bar_shell`, shared by every viewer.

What Eagle does, from the detail mode in `index.html` and `app.bundle.js`:

| Part | Eagle | Status | Falcon |
|---|---|---|---|
| Placement | `content-panel detail-mode` between the sidebar and inspector widths; the main toolbar and grid are hidden while it shows | Seen | Same |
| Panels button | `toggle-all-btn` at the left of both the main toolbar and the detail toolbar. `toggleAll` shows both panels when the sidebar is hidden and hides both otherwise; it doesn't remember each panel's state. Right-click lists both panels (Tab), the sidebar and the inspector | Seen | Same button in both bars, same rule (`logic::toggle_all`) and the same three-entry menu |
| Panel keys | View menu: ⌘⌥1 sidebar, ⌘⌥2 inspector, ⌘⌥3 both (`view.toggle.all`); Tab is a fixed key for both | Seen | Same keys, in the grid and in the viewer. ⌘⌥3 can be rebound; Tab can't and is ignored while a text field has focus |
| Counter | "n / total" after the exit button | Seen | Same, counted in the grid's order (`logic::viewer_counter`) |
| Previous and next | Buttons at the right end of the detail toolbar, disabled on the first and last item, and the arrow keys | Seen | Same |
| At the ends | A short "already the first / last item" tip in the middle of the viewer | Seen | A status note in the middle of the viewer for about a second (`logic::viewer_step`) |
| Double-click | Leaves the viewer anywhere in the image viewer, the picture included, except while cropping. Video and audio stop the double-click; PDF, font and text viewers are iframes that keep it | Seen | Same for the image viewer (also not in comment mode). Before, falcon reset the zoom on double-click; Fit and 0 still do |
| Right-click | Opens the item's context menu (`mouse-gesture.js`) | Seen | The grid card's menu, without Open, in the image viewer and over the audio and font viewers' background. Trashing or restoring the viewed item moves the viewer to the next item |
| Zoom keys | `view.zoom.in` ⌘=, `view.zoom.out` ⌘−, `view.zoom.actual` ⌘0, `view.zoom.fit` ⌘9 | Seen | Same chords and ids, rebindable; falcon also keeps plain 0, 1, + and − (without ⌘, ⌃ or ⌥) |
| Cannot preview | An illustration, the file name, a line saying the format can't be previewed and an "Open With Other Application…" button that opens the item menu | Seen | The file icon with its extension, the name, a line and an "Open With Another App…" button whose menu has Open in Default App, Open With and Show in Finder. Shown only when neither a full render nor the thumbnail can be shown; a failed full render falls back to the thumbnail |
| Not built | Presentation mode, always on top, thumbnail backgrounds, grayscale, the navigator, the separate preview window, Space's inline quick preview and mouse gestures | Seen | Not built |

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

## Video viewer

Opening a video AVFoundation can play (mp4, mov, m4v and the other QuickTime and MPEG-4 files with H.264, HEVC, ProRes and similar codecs) shows it with a control bar instead of the image viewer. Files it can't play (WebM, Matroska, AVI, WMV, FLV) fall back to the image viewer with their thumbnail. Playback, decoding and sound are macOS's own, through the `falcon-video` crate (`crates/falcon-video`), which keeps every Objective-C call in one module and hands the app RGBA frames to upload as a texture. Nothing GPL or LGPL is linked, so falcon stays MIT/Apache-2.0. The drawing, keys and timed comments are in `crates/falcon-app/src/app/video_view.rs`.

What Eagle does, from the `mediaElement` and `mpvMediaElement` directives and the comment dialogs in `app.bundle.js`, its stylesheets and `js/default-preferences.js`:

| Question | Eagle | Seen or inferred | falcon |
|---|---|---|---|
| Player | video.js over an HTML video element, falling back to a bundled mpv when the element can't decode the file (no size or duration, or no frame within 1.5 s of playing) | Seen | AVFoundation's AVPlayer, with frames copied out of an `AVPlayerItemVideoOutput` each time the window draws. No second player: what macOS can't play shows its thumbnail |
| Control bar | Play, back and forward, time, progress bar, volume, playback rate, loop, note (add comment) and full screen | Seen | Play, back, forward, previous and next frame, elapsed / total, add comment, speed, loop, mute and volume. No full screen |
| Picture | Fitted to the window and never enlarged past the video's own size (`max-width`/`max-height` set to the video size), unless the Zoom to Fill preference is on | Seen | Fitted and never enlarged; no fill mode. The size comes from the file, so records with `width` and `height` of 0 still show at the right shape. Rotated phone clips are shown upright |
| Start | Plays at once (`video.autoPlay`, on by default) and resumes where the item was left (`video.rememberPosition`, on), kept per item in local storage | Seen | Plays at once and resumes where the item was left during this session; positions are not kept across launches. A clip left at its end starts from the top |
| Loop | Clips of 30 s or less always loop (`video.loopShortVideo`, on); longer ones follow the loop button, whose state is saved | Seen | Same, and the loop setting is saved with the app's state |
| Volume | Saved in local storage as a percentage, 100% by default; a muted player saves 0% | Seen | Saved with the app's state, 100% by default. Mute is saved separately, so unmuting brings the old volume back |
| Speeds | 0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 3, 4 and 8×, from a menu or ⇧. and ⇧, | Seen | Same list from the speed menu or the keys. Each video starts at 1× (inferred: the rate belongs to the video element) |
| Skipping | ⌘← and ⌘→ move 2% of the length, at most 10 s. The back button moves the same; the forward button 2% at most 20 s; both repeat while held | Seen | Same steps; the buttons don't repeat while held |
| Keys | The `player.*` keybinds (play and pause, frames, volume, skip, speed) form their own Preferences group whose conflict scope is `player` | Seen | The same defaults in a Player group of Preferences → Shortcuts, N included (see Shortcut settings) |
| Frame steps | `[` and `]` one frame, ⇧[ and ⇧] ten frames. The HTML player moves by a measured frame time (1/24 s until measured); mpv steps real frames | Seen | Real frames, through AVPlayerItem's frame stepping. Stepping pauses |
| Seek bar | Click or drag; playing pauses while dragging and resumes after. Hovering shows a thumbnail preview of that moment. A resizable range on the bar limits playback to a section | Seen | Click or drag, pausing while dragging. No hover thumbnail and no range |
| Scrolling | Scrolling over the video seeks (the `habits.videoScrollBehavior` default, "progress"): 2% of the length at most 10 s per wheel step, less for small trackpad moves; scrolling up or right goes back | Seen | Same direction and steps. No volume mode |
| Clicking | Click plays or pauses (video.js); double-click leaves the viewer | Seen (double-click); inferred (click) | Click plays or pauses |
| Subtitles | A `.vtt` or `.srt` next to the original with the same name is loaded and shown | Seen | Not supported |
| Leaving | The position is saved and the player destroyed | Seen | The player stops and is dropped when the viewer closes or moves to another item, or another library is shown |
| Timed comments | Stored in the item's `comments` with `duration` (seconds), `annotation` and `lastModified`, kept sorted by time | Seen | The same records, written with `Comment::at_time`; the library keeps them sorted |
| Markers | A 4 px tick per comment on the progress bar at `duration / length`; hovering shows the text | Seen | Same, with the time and text on hover. Clicking a tick seeks there; right-click offers Edit and Delete |
| Adding | The note button or N pauses and asks for the text in a dialog at the bottom of the window; an empty text adds nothing | Seen | The comment button or N pauses and opens a box above the control bar (Save, Cancel, ⌘↩, Esc); an empty text adds nothing |
| Inspector | Comments listed with their times; clicking one seeks the video there, editing seeks and pauses first, deleting offers an undo notice | Seen | Listed with their times; clicking one opens the video at that moment; edit and delete as for region comments (⌘Z undoes) |

## PDF viewer

Opening a PDF shows it in a reader instead of the image viewer: the pages in one continuous vertical column, with zoom, page navigation, find, text selection and a thumbnail/outline sidebar. Pages are drawn by macOS's PDFKit through the `falcon-pdf` crate (`crates/falcon-pdf`), which keeps every Objective-C call in one module and hands the app RGBA pictures. The reader is in `crates/falcon-app/src/app/pdf_view.rs`.

The file is opened, rendered, searched and selected only on a worker thread, never on the UI thread, so a PDF whose contents are still in iCloud (or a 1,000-page book) can't freeze the window. While it opens the viewer says "Opening…", or "Downloading from iCloud…" when the file is dataless (its `SF_DATALESS` flag is set, read without starting a download; `pdf_view::icloud_only` is the one place to switch to a shared iCloud helper). A file PDFKit can't read, or a password-protected one, shows a line saying why. Off macOS the image viewer shows the preview instead.

Rendering: the UI asks the worker for the pages in view and one on either side, at the screen's pixel density rounded up to steps of about 4%, so pages are never blurry at rest. Each page picture is capped at 16 MP (pdf.js's own canvas cap), so past roughly 400% on a Retina screen a page is shown enlarged rather than re-rendered sharper. Pictures live as textures in an LRU cache of 128 MP for pages and 16 MP for thumbnails. While the zoom is moving, pages keep their old picture scaled; 160 ms after it stops they are rendered again at the new scale.

What Eagle does, from `app/pdf-viewer/` (pdf.js 3.1.81 with Eagle's edits in `web/viewer.html` and `web/viewer.js`) and the detail view in `app.bundle.js` and `index.html`:

| Question | Eagle | Seen or inferred | falcon |
|---|---|---|---|
| Engine | pdf.js in an iframe (`pdf-viewer/web/viewer.html?path=…&locale=…&theme=…`), the same in the main window and the preview window | Seen | PDFKit, drawn by egui |
| Toolbar | pdf.js's: sidebar toggle, find, previous/next page, page number and count, zoom out/in and the zoom menu, print and the Tools menu. Eagle hides Open, Download/Save and the annotation editor buttons (`style="display: none"`) | Seen | Sidebar toggle, find, previous/next page, page number field and count, zoom out, zoom menu, zoom in. No print and no Tools menu |
| Tools menu | Kept: presentation mode, first/last page, rotate clockwise and anticlockwise, text-select and hand tools, page/vertical/horizontal/wrapped scrolling, spreads (none/odd/even), document properties | Seen | Not built: falcon has vertical scrolling without spreads, the text tool only, and no rotation or presentation mode |
| Layout | pdf.js defaults: vertical scrolling, no spreads, pages centred on the viewer background | Seen | Same: one column, 10 pt between pages, centred |
| Zoom | pdf.js's menu: Automatic (default), Actual Size, Page Fit, Page Width, 50–400%; zoom steps ×1.1 between 10% and 1000%. Automatic is page width, page fit for landscape pages, never above 125%; 100% shows a point as 4/3 CSS pixels | Seen | Same choices, steps, limits and meaning of 100%; pinch or ⌘-scroll zooms around the pointer |
| Sidebar | pdf.js's thumbnails, outline, attachments and layers, hidden on open unless the document or the view history asks for it (`sidebarViewOnLoad` −1) | Seen | Hidden on open; the sidebar button shows thumbnails, and its right-click menu switches to the outline. No attachments or layers |
| Find | pdf.js's find bar (⌘F) with Highlight All, Match Case, Match Diacritics and Whole Words checkboxes, all off by default; ↩ and ⇧↩ or ⌘G and ⇧⌘G step through matches and wrap; the count shows "n of m" | Seen | ⌘F opens a find bar under the toolbar: query, previous/next, match case, "n of m". All matches are always highlighted (as with Highlight All on), the current one stronger; diacritics are ignored; a new search starts from the current page. No whole-words option |
| Text | pdf.js's text layer (`textLayerMode` 1): drag to select, ⌘C copies | Seen | Drag on a page to select its text (within that page), ⌘C copies, a click clears |
| Links | Clicks on links in the document open in the default browser (`shell.openExternal`) | Seen | Not handled: links are not clickable |
| Keys | pdf.js's: n/j and p/k turn pages, Home/End and ⌘↑/⌘↓ go to the ends, ⌘+/⌘−/⌘0 zoom, ⌘F and ⌘G find, r and ⇧R rotate. Eagle's page adds a keyup listener that posts Esc, ← and → to the app, which close the viewer and move to the previous or next item | Seen | n/j, p/k, Home/End, ⌘↑/⌘↓, ↑/↓ (scroll a line), Page Up/Down, ⌘+/⌘−/⌘0, ⌘F, ⌘G/⇧⌘G. ← and → move between items and Esc closes, as in Eagle; Esc closes the find bar first when it is open, and ← → stay in the find field while it has focus. Space closes the viewer as for images (in Eagle it scrolls once the iframe has focus) |
| Remembered | pdf.js's view history in local storage (`pdfjs.history`, 20 files by fingerprint): page, zoom, scroll position, rotation, sidebar and scroll/spread modes, restored on open (`viewOnLoad` 0) | Seen | The last page and zoom of the 20 most recent PDFs, by item id, saved with the app's state and restored on open |
| Theme | `theme` in the URL: "light" picks pdf.js's light theme, anything else its dark one, which only recolours the frame (body `#2A2A2E`); pages stay white, never inverted | Seen | Pages are white on the viewer's background colour of the current theme |
| Large files | pdf.js defaults: range requests and streaming on, auto-fetch on (`disableRange`, `disableStream`, `disableAutoFetch` all false); renders the visible pages and the next one, keeping at least 10 page canvases | Seen (settings); inferred (that ranges matter little for a local file URL) | PDFKit reads the file lazily; the visible pages and one either side are rendered, in a pixel-budgeted cache |
| Comments | No region comments on PDFs: mousedown on the image is ignored for `pdf` | Seen | Same: the reader takes no comments |

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

## Duplicate check on import

Every import from the app (drops, ⌘V, files passed to the app, the collect window) checks whether each file is already in the library, while the preference is on. The import thread stages the file without the library lock, takes the lock only to list items of the same size and extension, and compares the bytes with the lock released (`import_check`, see [Import](import.md)). A match is not committed: the staged copy is held in a queue and the thread goes on with the next file. When the batch has finished, the window takes the queue into a dialog; the thread never waits for the window. The code is in `crates/falcon-app/src/import_dupes.rs` (thread side and decisions, with tests) and `app/dupes_ui.rs` (the dialog).

The dialog, "Already in the library (N)", shows the existing item's thumbnail, name and first folder, the new file's size and target folder, two choices, Use the existing item (the default) and Keep both, a "Do the same for all N files" box when more than one file is held, and Import and Don't Import buttons. Return imports with the chosen option and Esc skips; with ⌘ held either answers for every file. A click outside doesn't answer. Use the existing item puts the existing item in the import's folders with their auto-tags and moves its date added to now (`Library::use_existing_for_import`); if that item has been deleted or trashed meanwhile, the file is imported instead. Keep both commits the held copy as a new item. Don't Import drops it, removing the staged copy. Each answer is one undo step and shows a notice ("Used the existing items for 2 files"). Held files count as done in the progress bar's "Added N". Held files still waiting when their library is closed are dropped.

## Keys

These are the defaults. All but the arrows, Esc, Tab, Z and the viewer's C, ⇧R, ⇧F, 0, 1, + and − can be changed under Preferences → Shortcuts (see Shortcut settings); the video keys are its Player group.

| Key | Action |
|---|---|
| Space (or ↩) | Open or close the viewer. With audio or video in the viewer, Space plays or pauses instead (Esc or ↩ closes) |
| ← → ↑ ↓ | Move the selection, or go to the next item in the viewer (with a PDF, ↑ ↓ scroll the document). With ⌘ or ⌥, ← and → go back and forward instead (see below) |
| ⌘A | Select all |
| ⌘⌫ | Move to the Trash, or delete permanently when in the Trash |
| ⌘R (or F2) | Rename the selected item |
| ⌘F | Focus search |
| ⌫, ⌘← and ⌥← | Back. In the viewer, ⌫ closes the viewer (as in Eagle) and the two arrow chords do nothing; with a video playing, ⌘← still skips in the video. None of the three acts while a text field has focus |
| ⌘→ and ⌥→ | Forward. In the viewer they do nothing; with a video playing, ⌘→ still skips in the video. Not while a text field has focus |
| ⌘] and ⌘[, ⇧⌘] and ⇧⌘[ | Move the open folder (or open smart folder) up or down one place among its siblings; with ⇧, to the top or the bottom (Eagle's Edit ▸ Move Folder). Each move is one undo step |
| ⇧⌘R | Folder settings (name and auto-tags) for the open folder, unless it is locked (Eagle's Set Current Folder, `edit.folder.setting`) |
| ⌘= and ⌘− | Zoom the grid's cards, or zoom the image in the viewer |
| ⌘⇧N | New folder |
| ⌘C and ⌘V | Copy the selected files, or paste files or images |
| ⌘Z and ⇧⌘Z | Undo and redo library edits, up to 100 steps. Each frame's edits are one step, named after what was done ("Undid: Move 3 to Trash"). Deleting permanently or emptying the Trash can't be undone and clears the history. Text fields keep their own undo while focused |
| ⌘⇧F | Show or hide the filter bar |
| ⌘⌥1 and ⌘⌥2 | Show or hide the sidebar and the inspector, in the grid and in the viewer |
| ⌘⌥3 and Tab | Show or hide both panels together: both show when the sidebar is hidden, otherwise both hide (Eagle's `view.toggle.all`; Tab is fixed and does nothing while a text field has focus) |
| ⌥1 to ⌥4 | Waterfall, Justified, Grid and List layouts, as in Eagle |
| Esc | Close the viewer, or clear the selection |
| ⌘0 and ⌘9 | In the viewer, actual size and zoom to fit (Eagle's `view.zoom.actual` and `view.zoom.fit`) |
| 0, 1, + and − | In the viewer, zoom to fit, actual size, zoom in and out (only without ⌘, ⌃ or ⌥) |
| Hold Z | Show the hover preview of the card under the pointer |
| ⌘⇧K | Show or hide the collect window |
| ⌘L | Open the library menu (Eagle's `library.switch`) |
| ⌃⇧Tab and ⌃Tab | Previous and next open library (falcon's own) |
| ⌘, | Preferences |
| ⇧↩ | Open the selected items (or the viewer's item) in their default apps (Eagle's `view.file.opendefault`) |
| ⌘↩ | Show the selected items (or the viewer's item) in Finder (Eagle's `view.file.openfinder`) |
| C | In the viewer, turn comment mode on or off (Esc also leaves it) |
| ⇧R | In the viewer, rotate the image left |
| ⇧F | In the viewer, flip the image horizontally |
| Enter and Esc | While cropping, save (then confirm) or cancel |
| ⌘← and ⌘→ | With video in the viewer, skip back or forward 2% of its length, at most 10 s |
| ⌘↑ and ⌘↓ | With video in the viewer, volume up or down 5% |
| `[` and `]` (with ⇧, ten) | With video in the viewer, previous or next frame |
| ⇧, and ⇧. | With video in the viewer, slower or faster |
| N | With video in the viewer, add a comment at the current time |
| n/j and p/k, Home and End | With a PDF in the viewer, next and previous page, first and last page (⌘↑ and ⌘↓ too); ↑ ↓ and Page Up/Down scroll |
| ⌘+, ⌘− and ⌘0 | With a PDF in the viewer, zoom in, out and back to Automatic |
| ⌘F, ⌘G and ⇧⌘G | With a PDF in the viewer, find in the document and step through matches (↩ and ⇧↩ in the find field) |
| ⌘C | With text selected in a PDF, copy it |

## Saving

The app opens libraries in batched write mode (see [Write batching](library-format.md#write-batching)): edits made during a frame, including those from the HTTP API and background imports since the last frame, are written together at the end of the frame. Pending writes are also flushed when a library is closed and, for every open library, when the app saves its state (every 30 seconds and on quit). A failed write shows "Could not save".

Metadata backups follow Eagle's timing (see [Backups](library-format.md#backups)). When a flush writes `mtime.json` or `metadata.json`, a backup is scheduled 1 second later, and each further write restarts the wait. Every open library is also backed up every 30 minutes. Both run `backup::backup_library` on a background thread, so the copy never blocks the window, and a library never has two running at once. The copy goes into the library's `backup/` folder, is skipped when the newest backup has the same size, and at most 100 are kept. When a library closes or the app saves its state with a backup still waiting, it is taken right away on the main thread so a quit doesn't lose it. The timing lives in `crates/falcon-app/src/backups.rs`, whose `Schedule` takes the time as an argument and has unit tests; nothing in the tests waits on a timer.

Metadata backups have no setting: falcon backs up after a change the way Eagle does (Eagle has no preference for it either). An earlier "Back up metadata on change" checkbox that did nothing was removed; old saved state that has it still loads.

## Several libraries

Falcon can keep several libraries open in one window and switch between them; Eagle can't. Each open library is a workspace with all of its own state: where you are (scope, back and forward history, search, filters, sort and layout), the selection, the viewer, its undo and redo stacks, open dialogs and its image caches. Switching keeps all of it. Global state stays shared: Preferences, shortcuts, the recent list, zoom, panel visibility, the collect window and the HTTP API server.

- **Switching:** click the library name at the top of the sidebar, the ⇄ button in the sidebar header, or press ⌘L. The menu lists the open libraries first (name over its folder, dimmed, the shown one highlighted, × to close), then recent libraries that aren't open, then Open Library…, New Library…, Show in Finder, the collect window and Preferences. ⌃⇧Tab and ⌃Tab step through the open libraries.
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

How falcon differs: several libraries open at once (above) and its own ⌃Tab and ⌃⇧Tab shortcuts; one window rather than one OS window per library (Eagle doesn't have several windows either); the recent list is newest first and capped at 10, with no search, icons, pinning, reordering, Reload or Merge yet.

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
| General | Theme: Auto, Light, Lightgray, Gray, Dark, Blue, Purple | `theme` (`{name, color, css}`), default `DARK`; Auto is `{name: "Auto", css: "auto"}` | Stored as `theme` under Eagle's `css` names (`auto`, `lightgray`, …); picking one previews it at once, Cancel or Esc switch back |
| General | Show the sidebar, show the inspector | View menu toggles, not preferences | Same flags as the panel buttons |
| General | Ask what to do when an imported file is already in the library | `notification.notification.when.repeatImage` (Notification ▸ Notify me when importing duplicates), on by default, and only while the notification master switch is on | On by default; when off, such files are imported as new items without asking, as in Eagle (see Duplicate check on import) |
| General | Add IPTC and XMP keywords as tags | `general.IPTC`, off by default | Same: every import from the app (drops, ⌘V, files passed to the app, the collect window) reads it (see [IPTC keywords](import.md)) |
| Preview | Hover zoom | `habits.hoverZoom`, on by default; off removes the zoom button, but holding Z still works | Same |
| Developer | Run the HTTP API, first port | Eagle always serves on 41595 and only has an API token setting | On by default, from 41596; tries five ports. Ports whose range would include 41595, or below 1024, are refused. Changing either restarts the API |

What Eagle does, from `js/default-preferences.js`, `preferences.html`, `js/preferences.js` and `css/preferences.css` (all seen):

- Preferences live in one tree with groups `general`, `habits`, `shortcuts.keybinds`, `theme`, `notification`, `sidebar`, `screencapture`, `video`, `proxy`, `privacy`, `autoImport` and `developer`. Booleans are stored as the strings `'true'` and `'false'`, and choices as strings (`hoverZoom: "on"`).
- The window's sidebar (200 pt wide, 32 pt rows) has General and Sidebar, then Controls, Preview (the `habits` group), Screenshot and Shortcuts, then Notification, Password and Auto-Import, then the AI panels. A search box filters every panel by keywords.
- Save applies and closes, Apply applies and stays open, Cancel and Esc revert (including a previewed theme or zoom), ⌘↩ saves.
- The window's own size is set in Eagle's main process, which is not in the readable bundle; falcon's 720 × 500 pt is a guess (inferred).
- Not built in falcon: interface zoom, language, the sidebar item toggles, scroll and double-click habits, notifications, screenshots, password lock, auto-import, proxy and the preference search.

## Themes

Eagle has seven theme choices: Auto, Light, Lightgray, Gray, Dark, Blue and Purple, in that order in `js/preferences.js` (observed). falcon offers all seven in the same order. The code is in `crates/falcon-app/src/theme.rs`:
- `Theme` is the choice.
- `Palette` holds one theme's colours.
- `p()` returns the current palette.
- `set_theme` switches it and restyles egui.
- `follow_system` runs once a frame and keeps Auto in step with macOS.

**Auto (observed in `js/preferences.js` and `app.bundle.js`).** Eagle stores Auto as `{name: "Auto", css: "auto"}` and never puts `auto` on `body`. Wherever it needs the theme, it asks Electron's `nativeTheme.shouldUseDarkColors`: true shows **gray**, false shows **light** (not dark, and not lightgray). The Preferences window listens for `nativeTheme`'s `updated` event, and the main window recomputes it when the theme changes.

falcon resolves Auto the same way, from egui's system theme (`Context::system_theme`, filled from winit's `ThemeChanged` and the event loop's appearance at start-up). When the appearance is not known yet, Auto shows gray, matching egui's dark fallback. Under Auto, falcon sets egui's theme preference to System, so the window is not pinned to an appearance and macOS changes reach egui (winit drops `ThemeChanged` while a window has its own appearance, and egui pins it for a fixed Light or Dark preference). egui's dark style gets gray's colours and its light style light's, so egui swaps its widgets by itself, and `follow_system` updates the palette `p()` returns. Neither step asks for any permission. With a fixed theme `follow_system` is one atomic load.

**Where the colours come from (observed in `style/base/_color.scss`).** `body[theme=…]` sets `--color-theme`:
- dark `#18191c`
- gray `#37383c`
- blue `#0d1630`
- purple `#1C1424`
- light `--color-white` (`#ffffff`; the map's `color-theme-light #f3f4f6` is not used by it)
- lightgray `--color-theme-lightgray` (`#e3e4e6`)

The four dark themes share one block, and light and lightgray share another (`body[theme=light], body[theme=lightgray]`, and the `light` mixin in `style/_mixins.scss` covers both). So lightgray is light's overlays, text, `#0072ef` primary and folder colours on a grey base:

| Variable | Dark family | Light and lightgray |
|---|---|---|
| foreground (text, hover and active overlays) | `--color-lightgray` `#f8f9fb` | `--color-dark` `#2c2f32` |
| `--color-primary` | `#3297FF` | `#0072ef` |
| borders (secondary) | white at 8% | black at 10% |
| card ring (`--box-item-border-color`) | white at 5% | black at 10% |
| hover / active | foreground at 5% / 10% | foreground at 5% / 10% |
| inputs (`_toolbar.scss`, `_inspector.scss`) | black at 20% | white at 50% |
| sidebar (`_sidebar.scss`) | lightgray at 3% | dark at 3% |
| folder colours and "none" | `#FF6667` … `#FF99CC`, `#BDBEC0` | `#E55245`, `#CC782F`, `#C29343`, `#548164`, `#0097A7`, `#3987BF`, `#8A67AB`, `#CC6699`, `#50555B` |

The old per-theme stylesheets (`css/style_<theme>.scss`) differ only in `$black-color`, which feeds a few legacy classes. They play no part here.

**Flattening (inferred).** Each falcon colour is one of these layers, composited source-over onto the surface below it and rounded to opaque sRGB:
- `main` is the base.
- `panel` is the sidebar layer over `main`.
- Rows, buttons, dividers, fields and muted text sit over `panel`.
- The search field, card ring, card hover and disabled text sit over `main`.

Which layer stands for which falcon colour is my reading, not something the stylesheets say. It assumes vibrancy is off. The same model applied to dark reproduces every colour sampled from Eagle's window to within 2 per channel. The script is `derive.py` in the session scratchpad.

A worked example, gray's selected row:
1. panel = 3% `#f8f9fb` over `#37383c` = 0.03 × (248, 249, 251) + 0.97 × (55, 56, 60) = (60.8, 61.8, 65.7), which rounds to `#3d3e42`.
2. selected row = 10% `#f8f9fb` over that = 0.10 × (248, 249, 251) + 0.90 × (61, 62, 66) = (79.7, 80.7, 84.5), which rounds to `#505155`.

The same for lightgray, whose foreground is `--color-dark` `#2c2f32` = (44, 47, 50) on `#e3e4e6` = (227, 228, 230):
1. panel = 0.03 × (44, 47, 50) + 0.97 × (227, 228, 230) = (221.5, 222.6, 224.6), which rounds to `#dedfe1`.
2. selected row = 0.10 × (44, 47, 50) + 0.90 × (222, 223, 225) = (204.2, 205.4, 207.5), which rounds to `#cccdd0`.
3. field = 50% white over the panel = 0.5 × (255, 255, 255) + 0.5 × (222, 223, 225) = (238.5, 239, 240), which rounds to `#efeff0`; the search field is 50% white over main, `#f1f2f3`.
4. muted text = 60% `#2c2f32` over the panel = (115.2, 117.4, 119.4), which rounds to `#737578`. On the selected row it reaches 2.9:1, a little under the 3:1 falcon's tests ask of the other themes, so the test allows 2.8 on rows and buttons.

Only dark was checked against Eagle's window. The other five were not sampled from a running Eagle, because switching its theme would mean changing Eagle's preferences and taking screenshots. Treat their colours as stylesheet-derived. Dark keeps its sampled values, which is why it differs slightly from the model (for example, `main` `#17191d` against the stylesheet's `#18191c`).

| Constant | Dark (sampled) | Light | Lightgray | Gray | Blue | Purple | Layer |
|---|---|---|---|---|---|---|---|
| `MAIN` | `#17191d` | `#ffffff` | `#e3e4e6` | `#37383c` | `#0d1630` | `#1c1424` | `--color-theme` (observed) |
| `PANEL` | `#1f2023` | `#f9f9f9` | `#dedfe1` | `#3d3e42` | `#141d36` | `#231b2a` | sidebar 3% over main |
| `DIVIDER` | `#303134` | `#e0e0e0` | `#c8c9cb` | `#4d4d51` | `#272f46` | `#352d3b` | border over panel |
| `ROW_SELECTED` | `#353639` | `#e5e5e5` | `#cccdd0` | `#505155` | `#2b334a` | `#38313f` | active 10% over panel |
| `ROW_HOVER` | `#2a2b2e` | `#efefef` | `#d5d6d8` | `#46474b` | `#1f2840` | `#2e2634` | hover 5% over panel |
| `FIELD` | `#19191d` | `#fcfcfc` | `#efeff0` | `#313235` | `#10172b` | `#1c1622` | input over panel |
| `FIELD_BORDER` | `#2c2d30` | `#e3e3e3` | `#d7d7d8` | `#414245` | `#232a3c` | `#2e2934` | border over field |
| `SEARCH` | `#131416` | `#ffffff` | `#f1f2f3` | `#2c2d30` | `#0a1226` | `#16101d` | input over main |
| `BUTTON` | `#2b2c2f` | `#efefef` | `#d5d6d8` | `#46474b` | `#1f2840` | `#2e2634` | hover 5% over panel |
| `BUTTON_HOVER` | `#343538` | `#e5e5e5` | `#cccdd0` | `#505155` | `#2b334a` | `#38313f` | active 10% over panel |
| `CARD_BORDER` | `#242528` | `#e6e6e6` | `#cccdcf` | `#414246` | `#19223a` | `#27202f` | card ring over main |
| `CARD_HOVER` | `#3a3b3e` | `#dfe0e0` | `#c8c9cb` | `#545559` | `#30384e` | `#3d3644` | foreground 15% over main |
| `GUIDE` | `#37383b` | `#e0e0e1` | `#c9cacd` | `#505155` | `#2d354b` | `#3a3341` | "none" colour 15% over panel |
| `TEXT` | `#f8f9fb` | `#2c2f32` | `#2c2f32` | `#f8f9fb` | `#f8f9fb` | `#f8f9fb` | `--color-text-primary` (observed) |
| `TEXT_MUTED` | `#a2a2a4` | `#7e8082` | `#737578` | `#adaeb1` | `#9da1ac` | `#a3a0a7` | text 60% (`-tertiary`) over panel |
| `TEXT_VALUE` | `#cdced0` | `#55575a` | `#505255` | `#d3d4d6` | `#cacdd4` | `#cdcdd1` | text 80% over panel |
| `TEXT_DISABLED` | `#5a5b5e` | `#c0c1c2` | `#acaeb0` | `#717275` | `#535a6d` | `#5e5965` | text 30% over main |
| `PRIMARY` | `#3297ff` | `#0072ef` | `#0072ef` | `#3297ff` | `#3297ff` | `#3297ff` | `--color-primary` (observed) |
| `BRAND` | `#0072ef` | `#0072ef` | `#0072ef` | `#0072ef` | `#0072ef` | `#0072ef` | `--color-brand` (observed) |
| `ICON_RED` … `ICON_PINK`, `ICON_NONE` | dark set | light set | light set | dark set | dark set | dark set | the two `$colors` maps (observed) |

**How the code reads colours.** Drawing code asks for the current palette with `t::p()` (`t::p().main`, `t::p().text_muted`, …), a static lookup that is cheap enough per card. The `pub const` colours in `theme.rs` are the dark values and only seed the dark palette. A few greys that used to be hard-coded for a dark background (the toolbar icon tint, the zoom slider, list zebra rows, the type placeholder, swatch rings and the delete-button red) now read the palette too, so they stay readable on light. Overlays drawn on images, on the primary colour or on black scrims keep their fixed white, as they do not sit on a theme surface.

## Shortcut settings

Preferences → Shortcuts lists every rebindable command by group, with its chord. Clicking a chord starts recording: the next key press becomes the shortcut. Backspace alone clears it, Esc alone stops recording, and a letter, digit or symbol needs a modifier (function keys, Space, ↩, arrows, Home, End, Page Up/Down, Delete and brackets may stand alone). A chord another command already uses is refused with "Used by …" for 1.5 s, and the old chord stays. A changed row has a reset button, and Restore Defaults (after a confirmation) resets them all. The last group, Player, holds the video player's keys (play or pause, previous and next frame, back and forward 10 frames, volume, skip, speed and add comment). They act only while a video plays in the viewer, so, as in Eagle, they conflict only with each other: the player may use ⌘F, and its Space overrides the viewer's while a video plays. Player chords must match their modifiers exactly (⇧[ is not also [). A command can also have extra chords that work only while its binding is still the default: ⌘← and ⌥← for Back, F2 for Rename and Enter for the viewer. Because Backspace alone clears a binding in the recorder, bare ⌫ can't be recorded for Back; the reset button brings it back. When a newer falcon's default lands on a chord the user had already given another command, the saved chord wins and that default is left unbound (its extra chords still work if free). The table, the parser and the recorder rules are in `crates/falcon-app/src/shortcuts.rs`.

Changes are saved as a map from command id to Eagle's accelerator text, for example `{"edit.rename": "Command + R"}`, holding only the changed commands; an empty string means unbound. Ids reuse Eagle's keybind names where Eagle has the command (`file.create.folder`, `view.zoom.in`, `find.filter.toggle`, `app.preferences`, `player.prev1frame`, `player.step.forward`, …). The add-comment key N gets falcon's own id (`player.comment.add`): Eagle handles N outside its keybind table, so it can't be changed there. falcon has no thumbnail keys (`player.thumbnail.*`). When a key press matches several chords (⌘F and ⌘⇧F for ⌘⇧F), the one with the most modifiers wins. ⌘C, ⌘X and ⌘V reach egui as Copy, Cut and Paste events rather than key presses, so falcon maps them back; this also makes ⌘C in the grid work, which the old hard-coded check missed.

What Eagle does, from `js/default-preferences.js`, `js/preferences.js`, `js/services/shortcut-manager.js` and `js/directives/shortcut-input.js` (all seen):

- `shortcuts.keybinds` maps 150 names (`file.create.folder`, `view.zoom.in`, …) to Electron accelerators such as `CmdOrCtrl + Shift + N`. On load, `CmdOrCtrl` is rewritten to `Command` (macOS) or `Ctrl`. Platform pairs such as `edit.rename.darwin` (⌘R) and `edit.rename.win32` (F2) show only on their platform.
- The recorder writes modifiers in the order Ctrl, Command, Shift, Alt, then the key; letters and digits need a modifier; Backspace alone clears.
- Conflicts are checked within a group's `conflict` scope: player keys only clash with other player keys, everything else with everything else. A clashing chord is refused, the old one stays, and a note names the other command for 1.5 s.
- Restore Defaults asks first, then reloads the defaults (plugin shortcuts too). The window has a shortcut search box; falcon has no search.
- falcon uses ⌘R for rename, Eagle's macOS default, and keeps F2 as an extra chord. It keeps its own ids for commands Eagle lacks or names differently (`view.viewer`, `go.back`, `edit.select.all`, `edit.folder.arrange.*`) (inferred mapping). Eagle's `view.layout.grid` and `view.layout.waterfall` are attached to Waterfall and Grid respectively, while falcon's ids of the same names mean its Grid and Waterfall layouts; only the defaults follow Eagle (⌥1 Waterfall, ⌥3 Grid).

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
| Folder menu | New Folder, New Subfolder, Move Folder, Quick Access, Rename, Copy as Link, Auto-Tag, a Password submenu (Create, Edit, Remove, Lock Now), a Sort By submenu that sorts folders by title (see below), expand and collapse entries, Duplicate, Export, Show Subfolder Content, Change Icon (submenu), a colour row, Remove. Subfolder, auto-tag, duplicate and export are disabled while the folder is locked | Seen | New Subfolder (disabled while locked), Rename, Quick Access, Move to Top Level, Auto-Tags… (disabled while locked), a Password submenu, a Sort By submenu (folders by title), a Sort Items By submenu, Change Icon, a colour row, Delete |
| Sorting folders by title | The folder menu's Sort By submenu has four entries, in this order: Title (Current Level) (A→Z), Title (Current Level) (Z→A), Title (All) (A→Z), Title (All) (Z→A). Current Level sorts the list the clicked folder sits in (its siblings, or the top level), not its subfolders. All sorts the top level and then every folder's children. Each asks first ("sort the folder list by Title?", with a Sort Folders By Title button), then saves once. The Folders heading has no menu | Seen | The same four entries and meaning, worded as "Title, This Level (A→Z)" and so on (`organize::FOLDER_TITLE_SORTS`, `title_sort_parent`), with a confirmation. `Library::sort_subfolders` does the sort with a natural, case-insensitive title order; the change is one undo step. falcon also puts the Sort By submenu on the Folders heading's context menu, where This Level means the top level |
| Item sort menu | Not in the single-folder menu. It is in the multi-folder menu and in the toolbar's layout panel. Entries: Global Settings (no own sort), Date Added, Date Modified, Date Created, Title, Extension, File Size, Dimensions, Rating, Duration, Random, then Ascending and Descending. The layout panel also lists Manual for folders | Seen | Same entries, offered as Sort Items By in the folder and smart folder menus and in the toolbar's layout menu. Tags is offered too, since falcon's sort supports it |
| Toolbar sort in a folder | While a folder or smart folder is open, the toolbar shows that folder's sort. Picking a key writes `orderBy` to the folder; the global sort changes only outside folders. Global Settings deletes `orderBy` and `sortIncrease`. The direction is written to the folder only when it has an `orderBy`; otherwise it changes the global direction | Seen | Same |
| Random and Manual | Random shuffles a cached order. A refresh button, a filter change and a folder reload clear the cache. Manual sorts by each item's `order[folderId]`, falling back to the import time. Dragging items within a folder switches it to Manual | Seen. That every visit reshuffles is inferred | Random shuffles once per visit to the folder. Duration sorts by length, with items that have none last. Manual sorts by the folder's `order` map with the import-time fallback (`search::sort_manual`), in all four layouts. Manual exists only for folders: smart folders and the other views don't offer it. Random, duration and manual apply only when no search or colour filter is ranking the list |
| Reordering items by hand | Items can be dragged to a new place only inside a folder whose sort is Manual or Date Added (its own, or the global one when it has none), with no filter and no search (the `box-sortable` class on `body`). Each card then gets a 16 pt drop strip just outside its left and right edges, as tall as the card; the strip under the pointer shows a 2 pt primary border, a 30% primary fill and 3 pt corners. Selected and pinned cards get no strips. The left strip drops before the card, the right one after it. In the list layout the strips are 4 pt bars across the top and bottom of each row. Dropping writes new `order` values and switches the folder to Manual, ascending | Seen | Same rule (`organize::manual_sortable`). `logic::manual_drop` hit-tests every layout: a card's box (with its caption) widened by 16 pt on each side, left half before, right half after, the nearer card winning in a gap and the later one on a tie; list rows split at half height. The dragged cards take no drop. The strip is drawn in the gap between cards (as wide as the gap, up to 16 pt) rather than overlapping the next card. Dropping calls `Library::move_items_manually`, which switches the folder to Manual; a drop that leaves the order unchanged writes nothing. Dragging items onto a sidebar folder moves or adds them (see the Dropping items on folders row below). Making the hit area cover the card halves, not only the strips, is falcon's choice |
| Icon picker | A Change Icon submenu holding a wrapping grid of 24 pt icons. The first, blank cell is the default icon | Seen | Same layout. Each of Eagle's 161 names gets the closest Phosphor Light glyph (`folder_icons.rs`), and `number10` to `number20` are drawn as digits. Eagle's artwork is not copied |
| Colours | A separate row in the menu: a "none" circle (which deletes `iconColor`), then red, orange, yellow, green, aqua, blue, purple and pink, 16 pt wide with 8 pt gaps. The colour tints the sidebar icon (not the label) and the folder's guide lines at 25%. Dark-theme values: `#FF6667`, `#FF9F0A`, `#FFD60A`, `#30D159`, `#40CBE0`, `#00AAFF`, `#C499FF`, `#FF99CC`; "none" is `#BDBEC0` | Seen | Same row and tints (`theme::ICON_*`). New subfolders take their parent's icon and colour (done in the library) |
| Reordering by drag | While a folder is dragged, each row shows three drop strips. A 9 pt strip at the top inserts before the row and shows a 1 pt primary-colour line. A 10 pt strip across the name nests the folder and shows a primary-tinted box with a 1 pt border and 6 pt corners. A 9 pt strip at the bottom inserts after. Dropping a folder into itself or a descendant is refused | Seen | The top and bottom quarters of a row show an insertion line and reorder; the middle half shows the box and nests (`organize::drop_zone`). `organize::tree_drop` works out the move and `Library::reorder_folder` makes it. Dropping on the Folders heading still moves a folder to the end of the top level. ⌘] and ⌘[ move the open folder up or down among its siblings, and ⇧⌘] and ⇧⌘[ to the top or bottom, as Eagle's Edit ▸ Move Folder does; the same keys work on an open smart folder (`organize::arrange`) |
| Dropping items on folders | Inside a folder, dropping items on another folder moves them: they leave the open folder and join the target, and holding ⌥ adds them instead. From any other view a drop adds. A single item that is already only in the target is left alone. Trashed items are restored. The target's auto-tags are added to the items. A notice gives the count and links to the folder. A locked target asks for its password first | Seen in `classifySelected` | Same rule (`logic::folder_drop` decides, `logic::apply_folder_drop` makes the change for each item). When the open view is a folder, a drop moves the items out of it and into the target, and holding ⌥ adds them, keeping their other folders. From every other view (All, Uncategorized, a tag, a smart folder, Recently Used, the Trash and so on) a drop adds. Dropping on the folder that is already open does nothing. Trashed items dropped on a folder come out of the Trash. A notice reads "Moved 3 items to Logos" or "Added 1 item to Logos", and the whole drop is one undo step. The target's auto-tags (its own and its parents', see [Folder auto-tags](import.md)) are appended to every dropped item that lacks them, in the same step. Not done: the notice has no link to the folder, and a locked target does not ask for its password first |
| Folder settings and auto-tags | The folder menu's Set Auto-Tag (⇧⌘R, also Edit ▸ Folder ▸ Set Current Folder) opens a dialog with the folder name and a tag input labelled for auto-tagging, plus Save Changes and Cancel; ⌘↩ in the name saves, Esc cancels. Saving writes the name and the folder's `tags`. When the tag list changed, the folder's new tags are appended to every item in the folder or any folder below it (trashed items too), without repeats; removed tags stay on items. There is no inherit or apply-to-subfolders switch: the parents' tags always apply (see [Folder auto-tags](import.md)), and there is no preference | Seen in `AutoTaggingController` and `index.html` | Auto-Tags… in the folder menu, or ⇧⌘R on the open folder, opens Folder Settings: the name, the tags as removable chips and a field that adds tags on Return or comma, a note naming the tags inherited from the folders above, and Save and Cancel (⌘↩ saves, Esc cancels). Save goes through `Library::rename_folder` and `Library::set_folder_auto_tags`, which changes only the folder's `tags` key (any cached `extendTags` is left as it was) and appends the new list to items in the folder and below with Eagle's rule; a notice gives the count. Name and items are one undo step. The subfolder cards' menu has no entry yet |
| Lock icon | Folders with a password show a lock on their row: closed while locked, open once unlocked | Seen | Same, beside the count |
| Unlock screen | The content area shows an illustration, "Unlock to view Contents", "Hint: …" when a hint is set, and a password field. Enter checks the password, and a wrong one shakes the field. Touch ID and the licence code also unlock | Seen | Same without the illustration, Touch ID or licence code. A wrong password turns the field red |
| What a lock hides | Items in a locked folder, or in any folder below one, are left out of All, search and every other list. A locked folder's subfolders are hidden in the sidebar until it is unlocked | Seen | Same (`organize::hidden_folders`, `without_hidden`). An item filed in both a locked folder and another folder is hidden everywhere. Sidebar counts for All, Uncategorized and smart folders still include locked items. A locked folder's own count is hidden |
| How long an unlock lasts | Unlocking sets a runtime flag that is never saved, so a folder stays unlocked until Lock Now, a library reload or quitting. Leaving the folder doesn't re-lock it | Seen. That the flag isn't saved is inferred from the save code | Same: falcon keeps a set of unlocked ids for the session and clears it when a library opens. Creating or changing a password leaves the folder unlocked until Lock Now |
| Password dialog | New: Password, Enter again, Password Hints. Change: Old Password, New Password, Enter again, Password Hints. Remove: the current password (or the licence code) | Seen | Same fields (`organize::check_password_form`). Passwords with characters beyond Latin-1 are refused, because Eagle's `btoa` can't store them |
| Tag groups | The All Tags page has its own sidebar: All, Uncategorized, Starred, then "Groups (n)" with a + button and a drag-sortable list of groups. Each group row has a tag icon tinted with the group colour. The tag list is grouped A–Z by default; "Group by > Tag Group" shows a heading "Name (count)" per group with its tags under it, and an "Uncategorized" pseudo-group last. Each tag shows a dot in its group's colour. The + button creates "Untitled" and starts a rename. Dragging tags onto a group moves them there (with Alt, copies them). The group menu has Find items, Rename, Remove, Add to Library and a colour row. Removing a group keeps its tags, and asks first only if the group has tags. The main sidebar has no tag group section | Seen | The page has a Group by switch (Alphabet or Tag Group) and a New Group button. In Tag Group mode, each group gets a heading with its colour dot and count, followed by an "Ungrouped" section. Drag a tag onto a heading to move it into that group, or onto Ungrouped to take it out; the tag menu's Move to Group does the same. Double-click a heading, or use its menu, to rename it. The menu also recolours and deletes the group (keeping its tags, and confirming first when it has tags). Not yet built: the group sidebar and Alt-copy |
| Reordering tag groups | Groups are reordered by dragging them in the page's group sidebar (a sortable list), which rewrites the order of `tagsGroups` | Seen | No group sidebar, so the Tag Group headings themselves drag: the top half of another heading drops before it, the bottom half after, shown by a primary insertion line (`organize::group_drop`, `Library::reorder_tag_group`). The heading menu also has Move Up and Move Down, for when dragging past a long group is awkward. Each move is one undo step |
| Tag group descriptions | With one group selected in the group sidebar, an editable line sits under its heading: 13 pt secondary text with a placeholder, no line breaks, a hover tint, and a bottom border. It saves when the field loses focus, only if the text changed (typing also saves after a second's pause) | Seen | A description shows under its heading in muted 13 pt text whenever it is set. Add Description… or Edit Description… in the heading menu, or a double-click on the text, opens a field in its place; leaving the field saves a changed text (line breaks become spaces, `organize::clean_description`), Escape cancels. There is no save-while-typing. The field is separate from the inline rename, since falcon has no rename popover |
| Saved filters | A Saved Filter button at the right of the filter bar opens a menu. "Save this filter" asks for a name; it is disabled when there is no filter and no search. The saved filters are listed below it, and clicking one applies it. Each has a more button with Rename and Remove (no confirmation). The menu can be sorted by drag and searched. Applying a saved filter replaces the whole filter and runs its saved search | Seen | A Saved menu in the filter bar holds Save This Filter… and the list. Clicking an entry applies it, replacing the filter and the search. Each entry's ⋯ menu has Rename, Move Up, Move Down and Remove. Saving keeps every rule falcon's filter has, including dates, duration, camera and font status, and applying restores them and refills the Date range and Duration fields. A notice names any saved rules falcon can't apply yet (comments, the 4:3, 16:9 and custom aspect ratios, excluding Uncategorized, image and semantic search) |
| Nested smart folders | The smart folder tree has carets, guide lines and the same three drop strips (nest or reorder). Smart folders take an icon and colour from the same picker and colour row. The dialog has no parent choice: the New Subfolder entry in a smart folder's menu passes the parent. The + menu also offers a smart folder group | Seen | The same tree, drag rules and icon and colour menu, with New Subfolder… in the menu. falcon's dialog also has an Inside choice (the parent, with each entry's icon so groups stand out), an icon and a colour, so it can move an existing smart folder too. The menu's Rename edits the name in place in the sidebar (Enter or leaving the field saves, Escape cancels), and Edit Rules… opens the dialog |
| Smart folder groups | The + beside Smart Folders opens a menu: New Smart Folder and New Smart Folder Group. The group is created at the top level as "Untitled" with the `grid` icon and no conditions, opened, and put into rename. It shows the union of its children and no count badge. Its menu is the same as a smart folder's (Edit Rules included) | Seen | Same menu, creation, icon (a group whose icon is cleared still falls back to the grid, `organize::smart_icon`), union and missing badge, also in Quick Access. The group's menu has New Subfolder…, Rename, Quick Access, Move to Top Level, Sort Items By, Change Icon, the colour row and Delete Smart Folder Group; Edit Rules… is left out, since a group has no rules and the dialog would turn it into a smart folder. A new child of a group starts with the default smart folder icon rather than inheriting the grid |

## Comments on a region

In the viewer, comment mode (the speech-bubble button or C) lets you drag a rectangle over the image. When you let go, a small box opens for the text: Save (or ⌘↩) adds the comment, Esc or Cancel drops it. Regions of 10 image pixels or less on either side are ignored, as in Eagle. Each comment is drawn as a numbered outline; hovering one shows its text. In comment mode, clicking an outline edits its text and its × button deletes it. Dragging inside an outline moves it, and dragging its right edge, bottom edge or bottom-right corner (the three small squares) resizes it; the box stays inside the image and at least 10 image pixels on each side, and the new box is saved when you let go, as one undo step. Outside comment mode, a drag pans the image as usual. Grid cards show the number of comments in a blue disc at the top right. The code is in `comments.rs`.

What Eagle does, from the viewer's `comment-item` and `comments-container` directives, `js/annotation-preview.js` and the card template:

| Question | Eagle | Seen or inferred | falcon |
|---|---|---|---|
| Mode | A toolbar button with a count badge, shortcut C, hidden for video, audio, fonts, 3D models, web links, PDF and text | Seen | Same button, badge and key, hidden for the same kinds |
| New region | Drag a box; on release it is scaled to the original's pixels, rounded, kept only when both sides are over 10 px, and added at once with empty text; an editable box then opens on it | Seen | Same rule. The box is added when the text is saved, and is clipped to the image |
| Markers | Outlined boxes with a number badge and a × remove button; hovering shows the text in a floating box | Seen | Same |
| Moving and resizing | Dragging a box in comment mode moves it; jQuery UI's `resizable()` with its defaults (handles `e,s,se`, `minWidth`/`minHeight` 10 layout px) resizes it. The pointer offset is divided by the viewer's zoom; a resize is scaled by the image-to-layout ratio, so `x`, `y`, `width` and `height` can be fractional. Saved on mouse-up (`image-change`); a press without movement writes nothing. Moves are not clamped to the image | Seen in the `commentItem` directive and the bundled jQuery UI defaults | Same three handles and move. The box is clamped to the image (one sticking out from Eagle is pulled in on the next drag) and kept at least 10 image pixels a side. Values are written to two decimals, whole ones as integers. Saved on release through `update_comment`, one undo step per drag. Items without dimensions get no regions. The math (`adjust_region`, `drag_region`, `region_handle`, `with_region`) is unit-tested |
| Text | Stored as HTML from a contenteditable box (`<br>` for line breaks) | Seen | Stored as plain text. Eagle's HTML is shown with the tags removed (`<br>` and block ends become line breaks, common entities decoded). Eagle will show falcon's line breaks collapsed and would read a `<` in falcon's text as HTML |
| Card badge | `has-annotation` card class: a 20 pt primary-blue disc with a white border at the top right, showing the count. Its title was meant to list the texts with tags stripped, but `replaceAll` is given the pattern as a string, so it shows the raw HTML | Seen | Same disc, without the title |
| Inspector | Comments listed with their texts; clicking one opens comment mode and scrolls to it | Seen | Listed with edit and delete buttons; clicking a region comment opens the viewer in comment mode |
| Undo | Deleting a comment offers a short-lived undo notice | Seen | Part of the normal undo history |

## Rotate, flip and crop

The viewer bar has Rotate Left and Rotate Right and a Flip button for jpg, png, webp and bmp, and Crop for jpg, png and webp. Flip mirrors horizontally, or vertically with ⇧ held while clicking, and ⇧F flips horizontally, as Eagle's single flip button and `edit.image.flip` shortcut (`Shift + F`) do (seen in `preview-window.js` and `default-preferences.js`). Eagle's button also mirrors a playing video; falcon's appears for images only. The card menu offers Flip Horizontal and Flip Vertical as two entries, for every selected item in those formats. All of these rewrite the original (see [Crop and rotate](thumbnails.md#crop-and-rotate-observed)). The crop tool starts on the whole image. Drag its edges or corners, or drag inside it to move it; the size in pixels shows underneath. Save asks for confirmation, then overwrites the original. These edits can't be undone, so they clear the undo history. The rectangle math is in `crop.rs`.

## Open in other apps

The card menu has Open in Default App (⇧↩), Open With and Show in Finder (⌘↩); the inspector has an Open in Default App button above Show in Finder; every viewer's bar (image, audio, video, font, and anything else that calls `App::viewer_open_button`) has an open icon at the right with a caret that lists Open With. Files are handed to `/usr/bin/open` on a short-lived thread (`open_with.rs`): it needs no permission, never reads the file (so a dataless iCloud original is fetched by the app that opens it), and the UI never waits. The Open With list comes from `NSWorkspace URLsForApplicationsToOpenURL:` (macOS 12 and later; earlier systems get only the default from `URLForApplicationToOpenURL:`), asked about an empty stand-in file with the same extension in a temp folder, because LaunchServices answers nothing for a missing path, and cached per extension.

| | Eagle | Seen or inferred | falcon |
|---|---|---|---|
| Menu entries | Card menu and View menu: Open in Default App, Open in Other App, Reveal in Finder, plus Open in New Window and Open URL | Seen (`app.bundle.js`) | Open in Default App, Open With, Show in Finder in the card menu; no separate window or URL entry here |
| Keys | `view.file.opendefault` `Shift + Enter`, `view.file.openfinder` `CmdOrCtrl + Enter`, `view.file.openother` unbound, `view.file.opennewwindow` `CmdOrCtrl + O` | Seen (`default-preferences.js`) | ⇧↩ and ⌘↩, rebindable; no Open With key |
| Several items | Opens up to 40 selected files, each in its default app | Seen (`openFilesWithDefault`) | The same cap; one `open` call for all |
| Open With list | A submenu on macOS, for one item only: the default first, labelled "(Default)" and followed by a separator, then the others by name; Eagle itself left out. Built by a helper (`launchHandler`) run on an empty temp file with the item's extension, cached per extension | Seen (`get-associated-application`) | The same order, label and separator, falcon's own bundle left out, one entry per bundle path and per name |
| Opening in an app | `open -a APP FILE`, with a 10 s timeout | Seen (`openWithApplicationPath`) | `open -a APP -- FILE`, not waited on |
| Default app | Sent to the main process (`open-with-default`) | Seen (the message); inferred (Electron's `shell.openPath`) | `open -- FILE…` |

## iCloud Drive

Libraries kept in iCloud Drive with Optimise Mac Storage on have most originals and thumbnails evicted: `ls -lO` shows the flags `compressed,dataless` (seen on a real library), `stat` still reports the full size, and the first read blocks until iCloud has downloaded the file. `icloud.rs` tells these apart from `stat` alone: the `SF_DATALESS` bit (`0x40000000`) of `st_flags`, or a missing file with iCloud's older `.NAME.icloud` placeholder beside it.

Opening such an item in the viewer shows "Downloading from iCloud…" over the dimmed preview, with a moving bar, while a background thread asks iCloud for the file (`NSFileManager startDownloadingUbiquitousItemAtURL:error:`) and checks every 250 ms until the flag clears; then the item opens as usual (image, GIF, audio, video, font or PDF). An error from `NSURLUbiquitousItemDownloadingErrorKey`, or no answer within 20 minutes, shows the reason and a Try Again button. Files the ubiquity API refuses (another file provider) get a background read of their first byte instead. Moving to another item or closing the viewer stops the polling but never cancels or evicts anything at the system level. Progress is not shown as a percentage: NSURL has no progress key, and `NSMetadataQuery`'s would need a query per file (inferred to be not worth it). Local files take the same path as before with one extra `stat`.

Where originals are read:

| Where | Thread | With a dataless file |
|---|---|---|
| Grid, preview and full-size decodes, QuickLook renders (`loader.rs`) | Loader workers | A worker waits for the download; the UI does not |
| Viewer: GIF frames, `Video::open`, audio decoding, font specimen, PDF | UI | Held behind the download screen until local |
| Waveforms, duplicate scan, background imports | Background threads | Wait in their own thread |
| HTTP API | Server thread | `/api/item/thumbnail` returns a path and reads nothing. The add-from-path routes import while holding the library lock, so adding a dataless file through the API still holds the window until it has downloaded (not yet handled) |
| Rotate, flip, crop, Refresh Thumbnail, Refresh Palette | UI | Items still in iCloud are skipped, their download starts, and the status line says to try again |
| Drag out | UI | The drag image falls back to a plain square instead of reading a dataless thumbnail; the files are passed by path |
| ⌘C, Open in Default App, Open With, Show in Finder | UI | Paths only, never read |

How long downloads take depends on the file and the link (inferred: seconds for a thumbnail, minutes for a several-hundred-megabyte book).

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
