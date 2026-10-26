# Smart folders

Smart folders live in the library `metadata.json` as `smartFolders`. Falcon keeps each entry as raw JSON so it round-trips unchanged, and evaluates it through `falcon_library::smart`.

## Shape (observed in code)

```json
{ "id": "…", "name": "Red logos", "modificationTime": 1793000000000,
  "conditions": [ { "match": "AND", "boolean": "TRUE",
                    "rules": [ { "property": "type", "method": "equal", "value": "png" } ] } ],
  "children": [] }
```

The Assets fixture has no smart folders, so this shape comes from Eagle's code, not from a real file.

## Evaluation (observed)

- **Conditions:** all of them must pass. `boolean: "FALSE"` negates one.
- **Rules in a condition:** combined by `match`, which is `AND` or `OR`.
- **Nesting:** a child must also pass every one of its ancestors' conditions, so it narrows its parent.
- **No conditions:** a smart folder without conditions shows the union of its children.
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
- **Editing:** falcon's dialog creates top-level smart folders with one condition. Editing an existing smart folder isn't built yet.
