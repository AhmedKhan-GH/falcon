# Smart folders

Smart folders live in the library `metadata.json` as `smartFolders`. Falcon keeps each entry as raw JSON so it round-trips unchanged, and evaluates it through `falcon_library::smart`.

## Shape (observed in code)

The Assets fixture has no smart folders, so these shapes come from Eagle's code, not from a real file.

Eagle's save rebuilds each **top-level** entry with these keys, in this order. Unset keys are dropped:

| Key | Type | Notes |
|---|---|---|
| `id` | string | Same generator as item IDs |
| `icon` | string | Optional. A name from the folder icon picker (`falcon_library::model::FOLDER_ICONS`) |
| `iconColor` | string | Optional. `red`, `orange`, `yellow`, `green`, `aqua`, `blue`, `purple` or `pink`. No key means the default grey |
| `name` | string | NFC-normalised on save. Renames are cut to 1024 characters |
| `description` | string | `""` when empty |
| `modificationTime` | integer | ms. Set on creation and when the edit dialog saves. Icon, colour, sort and move edits leave it alone |
| `conditions` | array | See below |
| `children` | array | Written when present. The dialog always creates `[]` |
| `orderBy`, `sortIncrease` | string, bool | Optional. The smart folder's own sort, with the same keys as folders |

```json
{ "id": "…", "icon": "star", "iconColor": "pink", "name": "Red logos", "description": "",
  "modificationTime": 1793000000000,
  "conditions": [ { "rules": [ { "property": "type", "method": "equal", "value": "png" } ],
                    "match": "AND", "boolean": "TRUE" } ],
  "children": [] }
```

**Nested entries** aren't rebuilt. Eagle writes them as they sit in memory, minus `newFolderName`, `editable` and `$$hashKey`. A child made in the dialog is `{id, name, conditions, modificationTime, children, parent}`. `parent` is the parent smart folder's id, and keys set later (`icon`, `iconColor`, `orderBy`, …) follow in the order they were added. Eagle may also write runtime keys such as `imageCount` or `pinyin` into nested entries. Falcon doesn't add those (inferred to be harmless, since Eagle recomputes them on load).

## Editing (observed)

- **Create:** the dialog appends a new smart folder to the top level or to the chosen parent's children.
- **Edit:** the dialog replaces `name` and `conditions` and stamps `modificationTime`. It edits the stored conditions in place, so keys it doesn't touch stay. On opening it adds `boolean: "TRUE"` to any condition without one.
- **Icon and colour:** these are separate menu commands. Picking the blank icon or no colour deletes the key.
- **Drag onto a smart folder:** appends the dragged folders to its children and sets their `parent`. Dropping a folder onto one of its own descendants is refused.
- **Drag between siblings:** inserts the folder before or after the target in the target's list. Eagle tries to delete `parent` when the target is top-level, but the delete hits the array rather than the folder. The stale key only lingers on nested folders: top-level entries are rebuilt without it on save.
- **Delete:** removes the folder with its children. Every edit rewrites `metadata.json` with a fresh top-level `modificationTime`.

Falcon's API, all on `Library`:

| Call | Effect |
|---|---|
| `create_smart_folder(name, conditions)` | Appends a top-level entry in the rebuilt shape above |
| `create_smart_folder_in(parent, name, conditions)` | Same, or appends a nested entry in the dialog shape with `parent` |
| `create_smart_folder_group(parent, name)` | Appends a smart folder group (see below) |
| `update_smart_folder(id, name, conditions)` | Edits at any depth and stamps `modificationTime` |
| `rename_smart_folder(id, name)` | Changes only `name` (cut to 1024 characters), at any depth, as Eagle's sidebar rename does: `modificationTime` is left alone |
| `move_smart_folder(id, parent, index)` | Moves to a position among the new siblings. Sets `parent`, or removes it at the top level. Refuses cycles |
| `set_smart_folder_icon(id, icon, color)` | `None` removes a key. New keys go right after `id` on top-level entries, as Eagle's save places them, and at the end of nested ones |
| `set_smart_folder_order(id, order, increase)` | Sets or clears `orderBy` and `sortIncrease` |
| `delete_smart_folder(id)` | Removes the folder with its children |
| `set_smart_folder_description(id, text)` | Sets `description` at any depth: an existing key keeps its place, a missing one goes right after `name`; `modificationTime` is left alone |

`Library::smart_folders()` returns `SmartFolder` values with `icon`, `icon_color`, `description`, `order_by` and `sort_increase` read out for display.

## The dialog (observed)

Eagle's dialog lists every condition. Each has a row choosing all (`AND`) or any (`OR`) of its rules and whether they must be true or false (`boolean`), then its rules, one per row: property, method, value. The second and later conditions are labelled as AND-ed with the ones above. A new condition is `{"rules": [...], "match": "OR", "boolean": "TRUE"}` with one rule, name contains `""`. A new rule is `{"property", "method", "value"}`; `unit` follows for file size and duration, and `width` and `height` for a custom shape. Changing a rule's property resets its method and value. Eagle caps both lists at 30, won't remove a condition's last rule, and saves an empty text value as it is (such a rule never matches). Picked dates are stored as local midnight.

Falcon's dialog (`smart_dialog_ui`, with the rows in `organize_ui::smart_conditions_ui`) does the same, in its own words: each condition reads "Items must match / must not match all / any of these rules", with buttons to add and remove conditions and rules. It offers every property the evaluator knows (table above). Dates are typed as `YYYY-MM-DD`, or picked from a calendar that opens from the button beside each date field (it starts at the date in the field, else today, and a click on a day fills the field), `within` takes a day count, folders come from a checklist of the library's folders, tags are comma-separated, and colours have a swatch picker. A value that can't be stored (a missing number, a bad date or colour) shows a message and disables Save, where Eagle would store whatever the field held. Falcon also needs at least one condition, so the dialog can't turn a smart folder into a group. Under the rules, at the left of the buttons, a live count says how many items the rules would show ("12 items found"), counting the smart folder's ancestors' conditions too, as Eagle's dialog does. It counts on a worker thread (`smart_count.rs`, `smart::count_draft`) once the rules have stayed unchanged for 250 ms, over a snapshot of the library taken when the dialog first counts; while a newer count is pending the old number is dimmed, and rules that can't be saved show no count.

Saving never loses rules. The draft lives in `falcon_library::smart_draft`, which keeps the stored JSON next to what the dialog shows:
- **Untouched** conditions and rules are written back exactly as they were read.
- **Edited** ones are the stored object with only the changed keys replaced. Key order and keys falcon doesn't know survive. Unlike Eagle, falcon doesn't add `boolean` to a condition that lacks it unless you invert the condition.
- **Unsupported rules:** a rule with a property falcon doesn't know, a method that property doesn't have, a unit other than the ones listed, or a value of the wrong JSON type shows as a read-only "Unsupported rule" row with its JSON. It is saved verbatim until you remove it.
- **Date rules** keep their stored timestamp when only the method changes between `on`, `before` and `after`. Switching to or from `within` restarts the value (30 days), as in Eagle.

## Smart folder groups (observed in code)

A smart folder group has no flag of its own. Eagle's "new smart folder group" command (in the + menu next to the smart folder heading) appends a top-level entry with `conditions: []` and the `grid` icon, named "Untitled" in the UI language. It saves at once, then opens the group and starts a rename. The save writes it as `{"id", "icon": "grid", "name", "description": "", "modificationTime", "conditions": [], "children": []}`. The command only makes top-level groups, but a group can be dragged into another smart folder like any other entry.

Any entry without conditions behaves as a group:
- **View:** it shows the union of its children's items. With no children it shows nothing.
- **Badge:** it shows no count (Eagle's counter returns 0 for an entry without conditions).
- **Children:** each child still has to pass its own ancestors' conditions. A group adds none.

`SmartFolder::is_group()` is `conditions.is_empty()`. `create_smart_folder_group(None, name)` writes the top-level shape above. `create_smart_folder_group(Some(parent), name)` nests a group in the key order of Eagle's new-group object, with `parent` added: `id`, `name`, `modificationTime`, `children`, `conditions`, `icon`, `parent` (inferred from how Eagle writes nested entries as they sit in memory).

**How falcon differs:** Eagle's union checks each child with its plain rule test, which treats a child with no conditions as matching every item. So a group nested in a group would show the whole library in its parent's view, but only its own children's items when opened. Falcon uses the second reading in both places: a nested group is the union of its own children.

## Evaluation (observed)

- **Conditions:** all of them must pass. `boolean: "FALSE"` negates one.
- **Rules in a condition:** combined by `match`. `AND` stops at the first rule that fails and `OR` at the first that passes; any other value needs every rule to pass. A condition with an empty `rules` list passes.
- **Errors:** Eagle's whole test fails, negated condition or not, when it reaches a rule whose property it doesn't know, or a condition without `rules`. Falcon does the same.
- **Nesting:** a child must also pass every one of its ancestors' conditions, so it narrows its parent. Moving a child out of its parent stops the narrowing.
- **No conditions:** a smart folder without conditions is a group and shows the union of its children (see below).

Rules by property. Number rules take `value: [a, b]`, with `b` used only by `between`. Date rules take ms timestamps, except `within`, whose `a` counts days.

| Property | Methods | Notes |
|---|---|---|
| `name`, `url`, `annotation` (note), `folderName` | `equal`, `contain`, `uncontain`, `startWith`, `endWith`, `regex`, `empty`, `not-empty` | Text and value are lower-cased. An empty value fails every method but `empty` and `not-empty`. `folderName` passes when any of the item's folders passes; an item in no folder fails |
| `camera` | the text methods | `rawMetas.camera`, lower-cased. A photo without a camera fails every method, `empty` included |
| `comments` | the text methods | `empty` and `not-empty` look at the `comments` list. The others test every comment's `annotation` joined together, without lower-casing and without refusing an empty value. An item without the key fails them |
| `width`, `height`, `bpm` | `=`, `>=`, `<=`, `>`, `<`, `between` | An item without `bpm` never matches |
| `fileSize` | the number methods | `unit` `kb` divides by 1024, anything else by 1024² |
| `duration` | the number methods | `duration` in seconds; `unit` `h` or `m` converts it to hours or minutes, anything else keeps seconds. No duration never matches |
| `iso` | the number methods | Integer parse (JavaScript `parseInt`) of `rawMetas.isoSpeed` and of the value |
| `aperture`, `focalLength` | the number methods | The first number in `rawMetas.aperture` (`f/2.8`) or `rawMetas.focalLength` (`50.0 mm`) |
| `shutter` | the number methods | The second number in `rawMetas.shutter`: 250 for `1/250.0 sec`. A whole-second exposure (`2.0 sec`) has no second number and never matches |
| `createTime` (date added), `btime` (created), `mtime` (modified) | `on`, `before`, `after`, `between`, `within` | `btime` and `mtime` fall back to `modificationTime`. `before` and the end of `between` include the following 24 hours. `on` compares local calendar days |
| `timestamp` (date taken) | the date methods | Integer parse of `rawMetas.timestamp` |
| `tags`, `folders` | `union`, `intersection`, `equal`, `identity`, `empty`, `not-empty` | Folder ids for `folders` |
| `type` | `equal`, `unequal` | An extension or a type group |
| `rating` | `equal`, `unequal`, `contain` | No star, 0 and `"none"` all mean unrated. `contain` takes the stars as text (`"35"`); any `none` in it matches every item |
| `shape` | `equal`, `unequal` | `custom` compares the item's ratio with the rule's `width` and `height` |
| `color` | `similar`, `accuracy`, `grayscale` | A stricter colour test than the filter panel's: the first colour must cover at least 33% and be within 96 of the query on each RGB channel; then an exact match with either dominant colour, or a CIEDE2000 difference under 20 (`similar`) or 10 (`accuracy`) and a CIE76 difference under that plus 50, for each of the two dominant colours that covers more than 33%. The primary (`marked`) colour and the filter's white-background fallback don't count, as in Eagle |
| `fontActivated` | `activate`, `deactivate` | Whether the font is activated through Eagle, by the key `<postScriptName>_.<ext>` (see [Font](./search.md#font-observed)). An item without `fontMetas` fails both |

Eagle's quirks are kept:
- **File size units:** any unit other than `kb` is treated as MB.
- **Start and end matches:** `startWith` and `endWith` build a case-insensitive regex from `^` plus the value and the value plus `$`, without escaping, so the value is a pattern there too.
- **Lower-cased patterns:** for the lower-cased properties the regex pattern is lower-cased with the text, so `\D` acts as `\d`.

## How falcon differs

- **Regex engine:** Eagle runs JavaScript `RegExp`; falcon runs the Rust `regex` crate. It has no look-around or backreferences, so a pattern using them fails to compile and never matches (in Eagle it works). `\w`, `\d`, `\b` and case folding are Unicode-aware, where JavaScript without the `u` flag is ASCII-only. An invalid `regex` pattern fails the rule in both, but Eagle fails its whole test on an invalid `startWith` or `endWith` pattern, where falcon fails only that rule.
- **Installed fonts:** like Eagle, falcon counts a font as installed when a file named after it is in Eagle's font folder (`~/Library/Fonts/EagleApp`, read once per evaluation). Unlike Eagle, it also counts a font in falcon's own folder (`~/Library/Fonts/Falcon`), where falcon's font viewer activates fonts, so a font activated by falcon is installed for falcon's `fontActivated` rule and not for Eagle's. Fonts installed any other way count as not installed in both. There are no such folders on other systems, so there every font is "not installed".
- **The dialog:** see above for how falcon's differs from Eagle's.
