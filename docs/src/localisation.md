# Localisation

Falcon's interface comes in eight languages: English, Simplified Chinese,
Traditional Chinese, Japanese, Korean, German, Spanish and Russian, the
same set Eagle ships. The translations are falcon's own; none is taken
from Eagle. Preferences ▸ General ▸ Language picks one, or Automatic,
which follows the first of macOS's preferred languages (System Settings ▸
General ▸ Language & Region) that falcon has, else English. A change
shows at once, menu bar included, and Cancel switches back, as with the
theme. `FALCON_LANG=ja` (any locale tag) overrides Automatic for one run.

The code lives in `crates/falcon-app/src/locale/`, the files in
`crates/falcon-app/locales/`.

## How keys work

A string is keyed by its English text, which stays in the code:

| Call | Gives | Use |
|---|---|---|
| `tr("Move to Trash")` | `&str` | A fixed text |
| `trf("Moved to {folder}", &[("folder", &name)])` | `String` | A text with named placeholders |
| `tr_n(n, "{n} item", "{n} items")` | `String` | A count; picks the language's plural form and fills `{n}` |
| `tr_nf(n, "{n} item in {folder}", "{n} items in {folder}", &[("folder", &f)])` | `String` | A count with other placeholders |
| `tr_key("Folders")` | the same `&'static str` | Marks a label in a `const` or `static` table; it is translated where it is shown, with `tr(label)` |

The menu bar and the shortcut list keep their labels in tables
(`menu::MENUS`, `shortcuts::TABLE`) and translate them when they draw; the
catalogue test reads those tables directly.

Rules for a new string:

- Pass the whole sentence as one key, with `{name}` placeholders; don't
  join translated pieces. Format numbers and sizes into the value
  (`("size", &format!("{:.1} MB", mb))`).
- Translate only what is shown. Text that is saved, compared, used as an
  id, sent to the HTTP API or written to a library stays English; a
  label used for both keeps its English function and is translated at
  each place it is drawn.
- Keys are literals: the test finds them by reading the source, so the
  first argument of `tr`, `trf` and `tr_key`, and both forms of `tr_n`,
  must be plain string literals.

A word that needs two translations in two places gets a context: a
lower-case prefix and `::` in the key, as in `tr("tags::Uncategorized")`
(tags in no group) beside `tr("Uncategorized")` (items in no folder).
English shows the text after `::`; `en.json` lists the context key with
that text. The contexts in use are `tags::` (the tag sidebar and tag
picker), `match::` (the Any/All picker of the tag and folder filters),
`size::` and `shape::` (a custom export size and a custom aspect
ratio), `model::` (the 3D viewer's toggles), `switcher::` (the library
switcher's Open heading) and `pdf::` (the PDF properties' File row).

`{{` and `}}` stand for literal braces. An unknown placeholder is left as
it is, so a mistake shows instead of vanishing.

## The files

`en.json` lists every key; it is generated, never edited by hand. Each
other file maps keys to translations, sorted by key:

```json
{
  "Move to Trash": "In den Papierkorb legen",
  "{n} items": { "one": "{n} Objekt", "other": "{n} Objekte" }
}
```

A count's key is its English plural form, and its value an object of the
language's plural forms: `one` and `other` for German and Spanish, `one`,
`few` and `many` for Russian (1, 21: `one`; 2–4, 22–24: `few`; the rest:
`many`), `other` alone (or a plain string) for Chinese, Japanese and
Korean. A key missing from a file shows in English.

The files are built into the binary with `include_str!`, so there is
nothing to install, and they are parsed the first time their language is
shown.

## Adding or changing a string

1. Write the code with `tr`, `trf`, `tr_n` or `tr_key`.
2. Run `FALCON_UPDATE_LOCALES=1 cargo test -p falcon-app locale`. It
   rewrites `en.json` from the code, drops keys the code no longer uses
   from the other files, sorts them, and lists for each language the keys
   still missing, with their English text.
3. Add the translations and run `cargo test -p falcon-app locale` until it
   passes. Note new machine-written translations in `locales/REVIEW.md`.

The tests fail when `en.json` differs from the code, when a file lacks a
key, has one the code doesn't use, isn't sorted, drops or adds a
placeholder (German and Spanish may say "one" in words instead of `{n}`
in the `one` form, which only 1 takes), or misses a plural form.

Changing an English text changes its key, so every translation of it
must be written again; the test lists them.

## Adding a language

1. Add a variant to `Lang` in `locale/mod.rs` with its `code`, its own
   name (`native_name`), the tags that select it (`from_tag`), its file in
   `source`, its plural rule (`plural` and `plural_forms`, from CLDR) and,
   if San Francisco lacks its script, a system font in `locale/fonts.rs`.
2. Create `locales/<code>.json` with `{}` and add it to `Lang::ALL`.
3. Run the update command above and translate the keys it lists.

## Fonts

San Francisco has Latin, Greek and Cyrillic but no Chinese, Japanese or
Korean, so choosing one of those adds a macOS font after it the first
time it is shown: Hiragino Sans (Japanese), Apple SD Gothic Neo
(Korean), Hiragino Sans GB (Simplified Chinese) and Heiti (Traditional
Chinese), from `/System/Library/Fonts`. A font once added stays for the
session.

## What stays English

Every window, pane, menu, dialog, toast and viewer goes through the
catalogue. What stays English is what isn't interface text: names falcon
writes into a library ("New Folder", "Untitled", "Screenshot - …", saved
filter and group defaults, frame snapshots), the debug report and the
error log, product and format names (Eagle, Finder, PNG, BPM, ISO),
units and symbols (px, MB, EV, ★), the font viewer's specimen texts and
messages that come from macOS or a library error unchanged. The
Preferences search matches the translated pane names and block titles,
but each block's keywords are English. Dates and numbers keep falcon's
English formats.
