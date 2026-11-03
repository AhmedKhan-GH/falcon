# Search and filters

Eagle has no search index. Every search is a linear pass over the items held in memory, and so is falcon's (`falcon_library::search`).

## Keyword search (observed)

Eagle joins an item's searchable text into one lower-case string and tests each term as a substring. The text includes:

- the text content of a text item, and the camera (`rawMetas.camera`), always
- the name
- the extension with a leading dot, such as `.png`
- the URL, only for queries of at least 2 characters
- the notes
- the text of every comment, region and video comments alike
- for a font, its full name (the `zh` entry first, else `en`), and, for queries of at least 3 characters, its whole `fontMetas` as JSON
- the tags
- the name and description of every folder the item is in

There is no accent folding: `e` doesn't match `é`.

### Search Scope (observed)

The caret beside the magnifier in the search box opens the Search Scope menu. It has a checkbox each for Name, Folder Name, Folder Description, Extension, Tags, URL, Comments and Notes, all on by default. An unchecked part is left out of the search text. The text content, camera and font names are not in the menu and are always searched. falcon remembers the choice across launches, shared by every library (`SearchScope`, kept by `crates/falcon-app/src/search_state.rs`). `search_text_in` builds the text for a scope, and `search_text` uses every part.

### Search All

⌥⌘F (Find ▸ Search All) leaves the viewer, opens the All page and focuses the search box, so the search covers the whole library.

The query syntax:

| Syntax | Meaning |
|---|---|
| `cat dog` | both terms must match |
| `cat OR dog`, `cat \|\| dog` | either term matches |
| `-bird` | the term must not match |
| `"black dog"` | a quoted phrase, matched as one substring |
| `( … )` | a group |

Eagle doesn't evaluate a real expression tree. It flattens the query into three lists: terms that must all match, OR groups where one member must match, and terms that must not match. So `dog || (cat black)` means `dog` OR the literal text `cat black`. Falcon reproduces this flattening.

If the terms fail, Eagle makes one last attempt: it tests the whole query as a single substring.

## Typo tolerance (falcon only)

Eagle has no typo tolerance: an item matches only when the text is in it. falcon's app can add typo tolerance on top of Eagle's matching (`Query::fuzzy_score`) with Preferences ▸ General ▸ Search ▸ "Also find near misses when searching" (`Prefs::typo_search`). It is on by default, a deliberate falcon extra; turned off, the search box gives Eagle's exact-substring results. The HTTP API always keeps Eagle's exact behaviour.

When nothing matches a search or the active filters, the grid says so in the middle ("No Matches", with a hint to try another keyword or loosen the filters), as Eagle shows its own nothing-found message with a search illustration.

With the preference on:

- An item that matches the way Eagle matches costs 0 edits.
- Otherwise, each AND term, and one member of each OR group, must match the start of some word in the item's text within a small number of edits. The edits are insertions, deletions, substitutions and swaps of two adjacent letters (optimal string alignment distance).
- Words are runs of letters and digits, so `photography_01.jpg` has the words `photography`, `01` and `jpg`.
- A term may match a whole word, or a word prefix at least as long as the term. So `tigr` finds `tiger`, but not `tiramisu` through `tir`.
- The edit allowance depends on the term:

| Term | Edits allowed |
|---|---|
| under 4 characters | 0 |
| 4–7 characters | 1 |
| 8 or more characters | 2 |
| contains a digit, or is a quoted phrase | 0 |

- Common Latin accents fold in typo matching, so `creme` finds `crème`.
- NOT terms (`-bird`) stay exact.
- **Order:** exact matches come first, in the chosen sort order. Typo matches follow, fewest total edits first, and in the chosen sort order within the same count.
- **Speed:** a word is skipped without computing the distance when the term has more letters missing from the word entirely than its allowance. On 20,000 synthetic items a search takes 6–12 ms in a release build.

## Filter bar

The bar under the toolbar shows one chip for each filter that is pinned or on. Eagle's default pins are Color, Tags, Folders, Shape, Rating and Type. The + button lists every filter the library can use, each with a pin that keeps it in the bar or takes it out, and clicking a name opens that filter. Duration, BPM, Camera and Font are only listed when some item has a value for them. The pins are remembered across launches with the search scope. Chips can be dragged sideways to reorder them (a press starts a drag once the pointer has moved 10 points, so a click still opens the chip); the new order, kept as the full list of filters as Eagle's `eagle.filter.toolbar.orders` does, is remembered across launches and is also the order of the + menu.

A filter that is on always shows its chip, pinned or not, and a chip opened from the + menu or a key stays until its menu closes. The code is in `crates/falcon-app/src/ui/filter_bar/mod.rs` and `ui/filter_bar/media.rs`.

- **Clear** appears while any rule is on. It switches every rule off and also releases the Lock, as Eagle's reset does. The search text stays.
- **Clear Filter** (⇧⌥⌘F, Find ▸ Clear Filter) does the same as the Clear button.
- **Lock** appears while a rule or a search is on. Opening another view (a folder, a smart folder, a tag, All, the Trash, going back or forward) switches every filter rule off and empties the search box, unless the Lock is on. Opening the view that is already open changes nothing.
- **Filter Sidebar** (⌥⇧F) shows the sidebar and focuses its folder filter box.

Find ▸ Open Filter and the keys ⌥F (Folders), ⌥T (Tags), ⌥C (Color), ⌥S (Shape), ⌥R (Rating), ⌥D (Date) and ⌥E (Type) show the filter bar and open one filter's menu. The Size, Dimensions, Duration, Comment, Notes, URL, Font, BPM and Camera rows exist in the menu and under Preferences, Shortcuts, and have no key.

## Filter rules (observed)

Every active rule must pass.

- **Tags and folders:** each has Any, All and Exact logic. Eagle has an OR-mode quirk: under Any, an item that has an included tag passes even if it also has an excluded tag. The folder filter matches folder ids exactly and doesn't include subfolders.
- **Type:** accepts an extension, or a group such as video, audio, font, word, excel or powerpoint.
- **Size, width and height:** inclusive ranges.
- **Rating:** any of the selected star values, with 0 meaning unrated.
- **Shape:** see below.
- **Notes, URL and Comment:** has a value, has no value, or contains all of the given keywords.

### Comment (observed)

The rule reads an item's region and video comments (`comments[].annotation`). Has needs at least one comment, None needs no comment, and Keywords needs every keyword, case-insensitively, to occur in one of the comments (not necessarily the same one). Notes and URL use the same three choices over one text.

### Size and Dimensions (observed)

- Size takes a minimum and a maximum in KB or MB (1 KB is 1024 bytes, 1 MB 1,048,576 bytes). A bound may have a fraction; it is converted to bytes and truncated to a whole byte.
- Dimensions takes a minimum and a maximum for the width and for the height, in whole pixels (fractions are truncated).
- Each bound is inclusive and an empty bound is open.

### Shape (observed)

| Shape | Passes when |
|---|---|
| Landscape, Portrait, Square | width greater than, less than or equal to the height |
| Panoramic (landscape and portrait) | the long side is at least 2.5 times the short one |
| 4:3, 3:4, 16:9, 9:16 | `width / height` equals the ratio exactly, as floating-point numbers |
| Custom W:H | `width / height` equals `W / H` exactly |

- The ratios compare floats for equality, so 1366 × 768 is not 16:9 and 1920 × 1080 is.
- In the custom ratio a side left at 0 (unset) lets every item through.
- The ticked shapes are OR-ed, the custom ratio included.
- The menu lists a shape with its count and hides one with no items (unless it is ticked). `shape_counts` counts as Eagle does: only items with a width that are not audio or fonts, each in exactly one of landscape, portrait, square, panoramic landscape and panoramic portrait, and in at most one of the four ratios.
- Rating and Type hide options with a count of 0 in the same way (unless the option is on).

### Tags chip

The menu has the Any, All and Exact choice and a search box. Without a search, tags in a tag group come first, in the order of their first group, then the others, each in plain code-unit order. With a search the tags are ranked by name (see Name ranking below). A sidebar in the menu switches between All Tags, Selected (the included and excluded ones) and each tag group that has tags in the list. Untagged is a row of its own on the All Tags page. Select all, on a group's page, includes every tag listed there. A click includes a tag and a right-click excludes it. The chip reads the included names, then the excluded ones with a leading minus.

### Folders chip

The menu lists the folders that hold items, in tree order and indented by depth, with their counts, and Uncategorized first. A search box ranks them by name and shows them flat. A click includes a folder and a right-click excludes it, Uncategorized included. Excluding Uncategorized is `SetFilter::exclude_empty`, which drops items that are in no folder under Any, All and Exact alike. Under Any, an item passes when it has an included folder, even if it also has an excluded one, or when excludes are set and it has none of them; with `exclude_empty` set, an item in no folder counts as excluded. All and Exact need every included folder and none of the excluded ones.

### Type filter (observed)

`type_included` follows Eagle's order and its early answers:

1. An item whose own extension is included passes.
2. If the video group is included, the answer is final: the item passes only if it is a video. So including both video and audio lets only videos through.
3. A `url` entry takes bookmarks without a site. The youtube, vimeo and bilibili entries take bookmarks whose `medium` is that site.
4. If the audio group is included, the answer is final: the item passes only if it is audio.
5. The word, excel and powerpoint groups, then font.

An excluded extension or group, or an excluded `url` or site, removes an item. The video group holds ts, 3gp, mp4, mov, m4v, webm, mkv, avi and similar; audio holds mp3, wav, flac, ogg, aac and m4a; font holds ttf, otf, ttc, woff and woff2.

The menu lists the extensions present, in code-unit order, then the built-in groups, with counts and a search box. A bookmark with a site counts under the site. Eagle lists the font, video and audio groups but never counts them, so its panel hides them. falcon counts them (`type_counts`), so they show.

### Dates (observed)

Eagle's panel has two date rules: Date Imported reads `modificationTime`, and Date Modified reads `mtime`, falling back to `modificationTime` when `mtime` is 0. All times are integer milliseconds. There is no Date Created rule in the panel; `btime` is only used by smart folders. Falcon adds a Date Created rule (`Filter::created`, reading `btime` with the same fallback) for the app; that part is falcon's own.

Each rule has presets, a calendar range and a month list:

| Preset | Passes when |
|---|---|
| Today | the time is after local midnight today |
| Yesterday | the time is after local midnight yesterday and before local midnight today (both ends exclusive) |
| Last 7, 30, 90, 365 days | `now − time` is less than N × 24 hours |

- "Yesterday" is midnight minus exactly 24 hours, so a DST change shifts it by an hour. Eagle's midnight also keeps the current milliseconds (it calls `setHours(0, 0, 0)` without the ms argument); falcon uses exact midnight.
- The range holds the local midnights that start the first and last picked days. An item passes when `start ≤ time ≤ end + 24 h`, so the last day is included whole, plus the exact next midnight. The range is ignored unless both ends are set.
- The presets and the range are OR-ed: any one of them is enough.
- The month list (`YYYY/MM` keys, local time) is a separate rule. When months are selected, the item's month must be one of them, in addition to the presets.
- Eagle's Date Modified range reads the Date Imported range's values, which looks like a bug. In the app both rules share one picker, so the effect is the same. Falcon gives each rule its own range.

### Duration (observed)

- Reads `duration`, in seconds, which may have a fraction.
- Min and max are entered in seconds, minutes or hours. Each bound is converted to seconds and truncated with `parseInt`. The test is `duration ≥ min` and `duration ≤ max`, both inclusive.
- Any numeric bound, including 0, makes the rule active. Items with no `duration` then fail.

### BPM (observed)

- Reads `bpm`, the tempo of audio items, which may have a fraction.
- Min and max are each truncated with `parseInt`. The test is `bpm ≥ min` and `bpm ≤ max`, both inclusive.
- Any numeric bound makes the rule active. Items with no `bpm`, or `0`, then fail.
- In saved filters the rule is `bpm: {min, max}`, with each key present only when set. Dates, duration, camera and font are saved too; see the `saved-filters.json` table in [Library format](library-format.md).

### Camera (observed)

- Reads `rawMetas.camera`, which Eagle only writes for RAW photos (see [Import](import.md)).
- The options are the distinct camera values across all items, sorted by plain JavaScript string comparison, which is case-sensitive.
- Selected cameras are OR-ed. The match is exact and case-sensitive.

### Font (observed)

The font rule is not a family picker. It has two choices, activated and deactivated, which test whether Eagle has installed the font:

- When a font is activated, Eagle copies it into `~/Library/Fonts/EagleApp/` on macOS. At startup it lists that folder and records `<base name>_<.ext lower-cased>` for each file.
- An item's key is `<postScriptName>_.<ext>`. The `postScriptName` is the first language value in `fontMetas.postScriptName`.
- Activated passes items whose key is in the set, and deactivated passes those whose key isn't. Items without `fontMetas` fail both.

### How falcon does it

`Filter` has `imported`, `created` and `modified` (`DateFilter`), `duration` (a pair of optional bounds in seconds), `bpm` (a pair of optional bounds, read from and written to saved filters; `search::bpm(item)` reads the value), `cameras` (a set) and `font` (`FontFilter::Off`, `Activated(keys)` or `Deactivated(keys)`).

- A `DateFilter` takes an optional `Clock` (`now` and the start of today). With none, it reads the system clock and time zone. `local_day_start` builds range ends from a date picker.
- `DurationUnit::to_seconds` converts the min and max fields.
- These helpers build the panel's options: `distinct_cameras`, `month_counts`, `duration_range`, `installed_font_keys_in` with `font_dirs` (Eagle's `EagleApp/` and falcon's `Falcon/` under `~/Library/Fonts`, the second a deliberate difference), and `distinct_font_families` (`fontMetas.fontFamily`, a falcon extra).

The Color filter's menu opens with a picker: a square where the horizontal position sets saturation and the vertical position sets brightness for the current hue, and a vertical strip beside it that sets the hue. Dragging either one changes the filter colour as it moves, and the hue is kept when the brightness reaches black. Below it are the swatches, then the hex field with a small square showing the current colour, the accuracy slider, the Grayscale only box and Clear.

The swatches are exposed to assistive technology as radio buttons named after their colours (Red, Orange, …, Moss), with their selected state, and a press on one picks the colour as a click does. The picker's square and strip are named too. The folder colour rows, the inspector's palette swatches (named by hex) and the welcome flow's themes are exposed the same way.

A popup takes every pointer click inside it: egui hit-tests by layer, so a click on a swatch or in the gaps between swatches never reaches the grid underneath. AccessKit's own hit test walks its tree instead, last child first, and egui builds that tree in creation order, so on its own it finds a card drawn later in the frame before the popup above it. `crates/falcon-app/src/access_order.rs` sorts each tree update by layer order before it leaves the app, and gives a menu or dialog its bounds so a press between its controls stops there. Two windows in the same layer order keep their creation order, as egui does not publish their stacking.

## Colour search (observed)

Eagle compares the query colour with the item's two dominant palette colours, then with any colour marked as the item's primary colour.

1. Items with no palette, or whose first colour covers less than 25%, never match.
2. An exact RGB match with either of the two colours matches at once.
3. Otherwise both colours are converted to CIELAB. A colour matches when its CIEDE2000 difference is below the accuracy setting (default 20, range 5–40) and its CIE76 difference is below the accuracy plus 50. Each dominant colour is only tested if it covers more than 33%.
4. Failing those, a palette colour marked as primary (`marked: true`, set with the inspector palette's Primary Colour toggle) matches when its CIEDE2000 difference is below 30, whatever its share of the image and whatever the accuracy setting. Its CIE76 distance is the sort key. Rule 1 still applies first.

Smart folder colour rules don't look at the primary colour, and have no white-background branch; see the Smart folders chapter.

Matching items are sorted by CIE76 distance, closest first.

Falcon's CIEDE2000 implementation is checked against the published reference values from Sharma et al. One of Eagle's branches, for mostly white backgrounds, can never run as the code is written. Falcon nests it inside the dominant-colour test, which is what it appears to be meant for. That is **inferred**.

## Name ranking (observed)

Eagle's tag and folder filter searches, and Go to Folder, rank names with a quick-search score (`string_score`). It is 1 for identical strings and 0 when the letters of the keyword don't all appear in the name in order. Otherwise it rewards letters that follow each other, letters that start a word and letters whose case matches, plus a bonus for a shared first letter. Lengths and positions count UTF-16 units, as in JavaScript.

`rank_names` applies it as Eagle does:

- Spaces are dropped from the keyword.
- Both sides are decomposed (NFKD), stripped of combining accents and lower-cased, so `e` finds `é`.
- Names that score 0 are dropped, and the rest are ordered best first, with ties keeping their order.

Eagle also scores the pinyin of Chinese names and matches traditional against simplified characters. falcon implements neither.

## Sorting (observed)

| Key | Natural direction |
|---|---|
| Date Imported (the default), Created, Modified | newest first |
| Title, Extension, Tags | A–Z, with numbers compared by value (`img2` before `img10`) |
| Size, Dimensions, Rating | smallest first |
| Duration | shortest first, by whole seconds (`parseInt`), with a missing duration as 0 |
| Random | shuffled once per visit |

Eagle's `sortIncrease` flag reverses this direction when it is off. The Trash is always sorted by deletion time, newest first.

## Saved filters

A saved filter stores the filter's rules and the search text in `saved-filters.json` (see [Library format](library-format.md)). Applying one replaces the whole filter and the search, and refills the fields of the filter menus. The rules falcon reads and writes include:

- comments, under `annotation` (has, none or keywords)
- the ratio shapes `43`, `34`, `169` and `916`, and the custom ratio as `custom` with `width` and `height`
- excluded Uncategorized, as `folder.excludes.NoFolders`, next to the excluded folder ids
- tags, folders, type, size, dimensions, rating, notes, URL, colour, dates, duration, BPM, camera and font
- Eagle's AI searches, `image` and `semantic`, which AI Search applies; until it can, they are kept and the filter bar says why (see the App chapter)

## Go to Folder

⌘J (Find ▸ Go to Folder…) and a bare J open a switcher near the top of the window. Pressing the key again closes it, and reopening it keeps the last keyword, selected. The code is in `crates/falcon-app/src/quick_switch.rs` and `ui/dialogs/quick_switch.rs`.

- **Tabs:** Folders, Tags, Smart Folders and Items. Tab moves to the next tab and ⇧Tab to the previous one, both wrapping.
- **No keyword:** the Folders and Smart Folders tabs list the whole tree in order, indented by depth, with the parent's name muted at the right. The Tags tab lists tags with grouped ones first, then the others, each in plain string order. The Items tab lists every item that is not in the Trash.
- **Recents:** when there are more than 15 folders (more than 10 smart folders) and some history, up to 10 recently opened ones that still exist come first. They are listed again in the tree below, so they show twice.
- **With a keyword:** folders, smart folders and tags are ranked by name (see Name ranking) and shown flat. Items are searched with the same search text and query syntax as the search box, with no typo tolerance.
- **Keys:** ↑ and ↓ move the highlight, Return opens the highlighted row and closes the switcher, Esc closes it, and a click opens a row. A folder or smart folder opens as a view; a tag opens its page; an item opens the first of its folders that exists (else All) with the item selected and scrolled into view.
- **History:** opening a folder or a smart folder moves it to the front of its list, which keeps the latest 200 and is remembered across launches with the other search settings, shared by every library.
