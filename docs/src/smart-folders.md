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
  "conditions": [ { "match": "AND", "boolean": "TRUE",
                    "rules": [ { "property": "type", "method": "equal", "value": "png" } ] } ],
  "children": [] }
```

**Nested entries** aren't rebuilt. Eagle writes them as they sit in memory, minus `newFolderName`, `editable` and `$$hashKey`. A child made in the dialog is `{id, name, conditions, modificationTime, children, parent}`. `parent` is the parent smart folder's id, and keys set later (`icon`, `iconColor`, `orderBy`, …) follow in the order they were added. Eagle may also write runtime keys such as `imageCount` or `pinyin` into nested entries. Falcon doesn't add those (inferred to be harmless, since Eagle recomputes them on load).

## Editing (observed)

- **Create:** the dialog appends a new smart folder to the top level or to the chosen parent's children.
- **Edit:** the dialog replaces `name` and `conditions` and stamps `modificationTime`.
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
| `move_smart_folder(id, parent, index)` | Moves to a position among the new siblings. Sets `parent`, or removes it at the top level. Refuses cycles |
| `set_smart_folder_icon(id, icon, color)` | `None` removes a key. New keys go right after `id` on top-level entries, as Eagle's save places them, and at the end of nested ones |
| `set_smart_folder_order(id, order, increase)` | Sets or clears `orderBy` and `sortIncrease` |
| `delete_smart_folder(id)` | Removes the folder with its children |

`Library::smart_folders()` returns `SmartFolder` values with `icon`, `icon_color`, `description`, `order_by` and `sort_increase` read out for display.

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
- **Rules in a condition:** combined by `match`, which is `AND` or `OR`.
- **Nesting:** a child must also pass every one of its ancestors' conditions, so it narrows its parent. Moving a child out of its parent stops the narrowing.
- **No conditions:** a smart folder without conditions is a group and shows the union of its children (see below).
- **Rule types:**
  - text: name, notes, URL, folder name
  - numbers: width, height, file size
  - dates
  - sets: tags, folders
  - type, rating, shape
  - a stricter colour test than the filter panel's

Eagle's quirks are kept:
- **File size units:** any unit other than `kb` is treated as MB.
- **Start and end matches:** `startWith` and `endWith` match without escaping the value.

## How falcon differs

- **Regex rules:** falcon has no regex engine, so these fall back to a substring test.
- **The `on` date rule:** falcon compares UTC days, while Eagle uses local days.
- **The app's dialog:** it creates top-level smart folders with one condition. Nesting, icons and moves are in the library API but not yet in the app.
