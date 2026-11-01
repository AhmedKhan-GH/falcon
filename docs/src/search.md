# Search and filters

Eagle has no search index. Every search is a linear pass over the items held in memory, and so is falcon's (`falcon_library::search`).

## Keyword search (observed)

Eagle joins an item's searchable text into one lower-case string and tests each term as a substring. The text includes:

- the name
- the extension with a leading dot, such as `.png`
- the URL, only for queries of at least 2 characters
- the notes
- the tags
- the name and description of every folder the item is in

There is no accent folding: `e` doesn't match `é`.

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

Eagle has no typo tolerance. falcon's app adds it on top of Eagle's matching (`Query::fuzzy_score`). The HTTP API keeps Eagle's exact behaviour.

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

## Filter panel (observed)

Every active rule must pass.

- **Tags and folders:** each has Any, All and Exact logic. Eagle has an OR-mode quirk: under Any, an item that has an included tag passes even if it also has an excluded tag. The folder filter matches folder ids exactly and doesn't include subfolders.
- **Type:** accepts an extension, or a group such as video, audio, font, word, excel or powerpoint.
- **Size, width and height:** inclusive ranges.
- **Rating:** any of the selected star values, with 0 meaning unrated.
- **Shape:**
  - landscape and portrait
  - square
  - panoramic, when one side is at least 2.5 times the other
- **Notes and URL:** has a value, has no value, or contains all of the given keywords.

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
- In saved filters the rule is `bpm: {min, max}`, with each key present only when set.

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
- These helpers build the panel's options: `distinct_cameras`, `month_counts`, `duration_range`, `installed_font_keys` with `eagle_font_dir`, and `distinct_font_families` (`fontMetas.fontFamily`, a falcon extra).

## Colour search (observed)

Eagle compares the query colour with the item's two dominant palette colours only.

1. Items with no palette, or whose first colour covers less than 25%, never match.
2. An exact RGB match with either of the two colours matches at once.
3. Otherwise both colours are converted to CIELAB. A colour matches when its CIEDE2000 difference is below the accuracy setting (default 20, range 5–40) and its CIE76 difference is below the accuracy plus 50. Each dominant colour is only tested if it covers more than 33%.

Matching items are sorted by CIE76 distance, closest first.

Falcon's CIEDE2000 implementation is checked against the published reference values from Sharma et al. One of Eagle's branches, for mostly white backgrounds, can never run as the code is written. Falcon nests it inside the dominant-colour test, which is what it appears to be meant for. That is **inferred**.

## Sorting (observed)

| Key | Natural direction |
|---|---|
| Date Imported (the default), Created, Modified | newest first |
| Title, Extension, Tags | A–Z, with numbers compared by value (`img2` before `img10`) |
| Size, Dimensions, Rating | smallest first |

Eagle's `sortIncrease` flag reverses this direction when it is off. The Trash is always sorted by deletion time, newest first.
