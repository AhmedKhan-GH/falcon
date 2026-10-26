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
