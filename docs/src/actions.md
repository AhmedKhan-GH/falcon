# Actions

An action is a named list of organising steps (add or remove tags, add to or remove from folders, set a rating) with an optional shortcut. Running it applies every step to the selected items. Each library has its own actions.

## What Eagle does

These findings come from Eagle 4.0.0's application bundle. The file and the byte offset where each was read are given in brackets.

- Eagle's own window does not run actions. A small class, `eagle.action` (`js/lib/api/ai-action.js`, also bundled into `app.bundle.js` at byte 58723), loads the list, shows it in the menu bar and hands each run to a plugin with the id `ai-action`, Eagle's AI Action plugin. When that plugin isn't installed, Eagle offers to install it. The plugin isn't part of the application bundle, so its editor, its step types and how it applies them can't be read there.
- The list is stored in the library, in `actions.config.json` at the library root (`initActions(rootDir)`, `ai-action.js`; called when a library opens, `app.bundle.js` byte 1184710). It is a JSON array. A missing file, or one that doesn't parse, is an empty list. Eagle polls the file every 4 seconds (`fs.watchFile`, interval 4000) and rebuilds the menu when its modification time changes. Eagle's window never writes the file.
- Eagle's window reads three fields of each entry: `id`, `name` and `shortcut`. The shortcut is an Electron accelerator. On Windows Eagle replaces `Command` with `CmdOrCtrl` (`formatShortcut`), so the stored text uses `Command`, as Eagle's keybind preferences do (`"CmdOrCtrl + Shift + N"` style, with spaces around `+`, `js/default-preferences.js`).
- The menu bar has a top-level **Actions** menu between View and Window. It is shown only while `eagle.action.actions.length > 0`. Each entry is an action's name with its shortcut as the menu accelerator. A click (or the shortcut) first asks "Confirm bulk action" when 30 or more items are selected (`checkOperationSafety2(count, …, 30)`). It then opens the plugin with `{ triggerType: 'shortcut', actionId }` (`app.bundle.js` bytes 1319438 to 1319966).
- The toolbar has an Actions button (tooltip "Actions" with the key G). It sits right of the plugin buttons and left of the layout button, and is hidden on the All Tags page (`index.html` line 630, `ng-hide="viewMode=='alltags'"`). It and the bare key G (`'g': $scope.openActionsPanel`, `app.bundle.js` byte 2344422) open the plugin's panel with the selected items' ids (`eagle.action.open`, byte 2070795).
- The item context menu has no Actions entry. Searching the bundle finds only the two references above to `openActionsPanel` and one to `eagle.action.open`.
- While the plugin's window is open, switching libraries is refused with "AI Action in Progress" (`app.bundle.js` byte 1812698).
- Eagle's translations still carry strings from its earlier built-in Actions (`i18n/en.js`): an Actions panel with Create Action, Edit, Duplicate and Remove, an empty-state introduction and "No Action Found" (`actionsPanel.*`), an editor with Name, Steps, Add Step and Shortcuts (`modal.newAction.*`), seven rule types (`createAction.rule.*`: Add tags, Remove tags, Clear tag, Add folders, Remove folders, Clear Folder, Rating) and a "Task completed" notice (`notify.actions.run`). No code in 4.0.0 uses them. A library made by an earlier Eagle can hold an `actions.json` file holding `[]`, which 4.0.0 doesn't read.

## What falcon does

Falcon runs actions itself. It keeps them where Eagle 4.0.0 looks for them, `actions.config.json` in the library, and uses Eagle's earlier rule types as its steps.

| Entry point | Behaviour |
|---|---|
| Actions menu | Between View and Window, only while the library has actions. Each item shows the action's shortcut. It is enabled when items are selected (or one is in the viewer), outside the Trash, and when falcon can run the action |
| Toolbar button | The lightning button left of the layout button opens the Actions panel under it. Hidden on the All Tags page, as in Eagle |
| G | Opens the Actions panel (`organize.actions.panel`, rebindable under Preferences → Shortcuts) |
| An action's shortcut | Runs it on the selection when no text field or dialog has the keys. The modifiers must match exactly, as with Eagle's menu accelerators |

The **Actions panel** lists the library's actions with a search box (every typed word must be in the name). Clicking an action runs it on the selection and closes the panel. Each row's ⋯ menu has Edit…, Duplicate (a copy named "… Copy" right after it, without the shortcut) and Remove… (asks first). New Action opens the editor. The foot of the panel says how many items are selected. A click outside the panel or Esc closes it.

The **editor** has a name, a shortcut button and the ordered steps.

- The shortcut button records the next key press under Eagle's recorder rules (see Shortcut settings in [The desktop app](app.md)). Letters and digits need a modifier, Esc stops recording and Backspace clears. A chord that another action or a falcon command already uses is refused, and "Used by …" names that action or command.
- Each step has a kind and its parameters. Add tags and Remove tags take tags typed into a field (separated by commas, Return adds them), with the library's matching tags offered below the field. Add to folders and Remove from folders take folders picked from the folder tree, with a search. Set rating uses the inspector's five stars. Clear all tags and Remove from all folders have no parameters. The arrows move a step up or down, and × removes it.
- Save stays disabled until the action has a name, at least one step, and tags or folders for every step that needs them.

Preferences → Shortcuts lists the open library's actions and their keys in an Actions group. The keys are set in the editor. A command can't be given a key an action uses.

### Running an action

- The steps run in order. Each step is applied to every selected item before the next step starts. "Clear all tags, then add *ref*" leaves only *ref*; the reverse leaves no tags.
- Items in the Trash are skipped, and actions don't run on the Trash page. Adding an item to a folder therefore never takes it out of the Trash.
- From 30 selected items the run asks first, as Eagle's Actions menu does.
- The whole run is one undo step, named "Run *name*". The notice after it says how many items changed and has an Undo button.
- Adding tags puts them at the front of the tag history, as the tag picker does. Adding to a folder also adds that folder's auto-tags. A folder that no longer exists is skipped, and the notice names how many were skipped. A failed write stops the run at that point. The steps already done stay in the same undo step.

### Entries written by Eagle

Falcon keeps every entry exactly as it read it. An entry that has fields other than `id`, `name`, `shortcut` and `steps`, or steps falcon doesn't know, is taken to be the AI Action plugin's. It is listed greyed in the panel and the menu, its shortcut is still reserved, and it can be removed. It can't be run, edited or duplicated in falcon. Saving another action leaves these entries byte for byte as they were.

## Storage format

`actions.config.json` is a JSON array at the library root. Falcon writes it only after an edit, atomically (a temp file in the library, then a rename), compact like Eagle's other library files. If the file it replaces was indented, falcon keeps the indentation (two spaces). If the file can't be read, falcon lists nothing and refuses to write over it. Before an edit, falcon reads the file again if its modification time has changed. While a library is open, falcon also checks the file every 4 seconds, so actions saved by Eagle show up.

Each falcon entry:

| Key | Type | Meaning |
|---|---|---|
| `id` | string | A new item-style id (see [Item and folder IDs](ids.md)) |
| `name` | string | The name shown in the menu and the panel |
| `shortcut` | string | An Eagle accelerator such as `"Command + Shift + 1"`, or `""` for none |
| `steps` | array | The steps in order, each an object tagged by `type` |

The step objects:

| `type` | Other keys | Effect |
|---|---|---|
| `addTags` | `tags`: array of strings | Adds each tag the item lacks (trimmed, at most 1024 characters) |
| `removeTags` | `tags`: array of strings | Takes these tags off |
| `clearTag` | none | Takes every tag off |
| `addFolders` | `folders`: array of folder ids | Adds the item to each folder, with the folder's auto-tags |
| `removeFolders` | `folders`: array of folder ids | Takes the item out of these folders |
| `clearFolder` | none | Takes the item out of every folder |
| `rating` | `rating`: 0 to 5 | Sets the `star` key, or removes it for 0 |

The `type` names follow the rule names in Eagle's translations. `clearTag` and `clearFolder` are singular, as there.

A sample, which the tests in `crates/falcon-library/src/actions.rs` read and write back byte for byte:

```json
[{"id":"MA1B2C3D4E5F6","name":"File as reference","shortcut":"Command + Shift + 1","steps":[{"type":"addTags","tags":["reference","to sort"]},{"type":"removeTags","tags":["inbox"]},{"type":"addFolders","folders":["MFOLDER000001"]},{"type":"rating","rating":4}]},{"id":"MA1B2C3D4E5F7","name":"Reset","shortcut":"","steps":[{"type":"clearTag"},{"type":"clearFolder"},{"type":"removeFolders","folders":["MFOLDER000002"]},{"type":"rating","rating":0}]}]
```

## Code

- `crates/falcon-library/src/actions.rs`: the file (`ActionsFile`, which keeps each entry's JSON), the steps (`Step`), and `Library::run_steps`.
- `crates/falcon-app/src/actions/`: `mod.rs` holds `ActionsState` (follows the open library, polls the file, finds shortcut conflicts), `app.rs` runs actions and routes the menu and the keys, `panel.rs` draws the panel and `editor.rs` the editor. A run goes through the reducer as `Action::RunSteps`, so it lands in the frame's undo step.
- `crates/falcon-app/src/menu.rs`: `ActionEntry`, `action_enabled` and the AppKit Actions menu.
