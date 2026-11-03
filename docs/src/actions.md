# Actions

An action is a named list of organising steps (add or remove tags, add to or remove from folders, set a rating, auto-tag or write a description with AI) with an optional shortcut. Running it applies every step to the selected items. Each library has its own actions. In Eagle 4.0.0 this is a plugin (`ai-action`); falcon builds it in and keeps the actions in its own app data.

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

Falcon runs actions itself, with Eagle's earlier rule types as its steps and two AI steps of its own (see [AI steps](#ai-steps)). It keeps them in falcon's own app data, not in the library, and never reads, creates, changes or deletes the library's `actions.config.json`, which belongs to Eagle's AI Action plugin. Actions made in Eagle's plugin don't appear in falcon, and falcon's don't appear in Eagle.

| Entry point | Behaviour |
|---|---|
| Actions menu | Between View and Window, only while falcon has actions for the open library. Each item shows the action's shortcut. It is enabled when items are selected (or one is in the viewer), outside the Trash, and when falcon can run the action |
| Toolbar button | The lightning button left of the layout button opens the Actions panel under it. Hidden on the All Tags page, as in Eagle |
| G | Opens the Actions panel (`organize.actions.panel`, rebindable under Preferences → Shortcuts) |
| An action's shortcut | Runs it on the selection when no text field or dialog has the keys. The modifiers must match exactly, as with Eagle's menu accelerators |

The **Actions panel** lists the library's actions with a search box (every typed word must be in the name). Clicking an action runs it on the selection and closes the panel. Each row's ⋯ menu has Edit…, Duplicate (a copy named "… Copy" right after it, without the shortcut) and Remove… (asks first). New Action opens the editor. The foot of the panel says how many items are selected. A click outside the panel or Esc closes it.

The **editor** has a name, a shortcut button and the ordered steps.

- The shortcut button records the next key press under Eagle's recorder rules (see Shortcut settings in [The desktop app](app.md)). Letters and digits need a modifier, Esc stops recording and Backspace clears. A chord that another action or a falcon command already uses is refused, and "Used by …" names that action or command.
- Each step has a kind and its parameters. Add tags and Remove tags take tags typed into a field (separated by commas, Return adds them), with the library's matching tags offered below the field. Add to folders and Remove from folders take folders picked from the folder tree, with a search. Set rating uses the inspector's five stars. Clear all tags, Remove from all folders and Auto-tag with AI have no parameters. Write a description with AI has Replace existing notes. The arrows move a step up or down, and × removes it.
- Save stays disabled until the action has a name, at least one step, and tags or folders for every step that needs them.

Preferences → Shortcuts lists the open library's actions and their keys in an Actions group. The keys are set in the editor. A command can't be given a key an action uses.

### Running an action

- The steps run in order. Each step is applied to every selected item before the next step starts. "Clear all tags, then add *ref*" leaves only *ref*; the reverse leaves no tags.
- Items in the Trash are skipped, and actions don't run on the Trash page. Adding an item to a folder therefore never takes it out of the Trash.
- From 30 selected items the run asks first, as Eagle's Actions menu does.
- The whole run is one undo step, named "Run *name*". The notice after it says how many items changed and has an Undo button.
- Adding tags puts them at the front of the tag history, as the tag picker does. Adding to a folder also adds that folder's auto-tags. A folder that no longer exists is skipped, and the notice names how many were skipped. A failed write stops the run at that point. The steps already done stay in the same undo step.

### AI steps

**Auto-tag with AI** adds the tags the default model suggests. **Write a description with AI** puts the model's description in the item's notes; an item that already has notes keeps them and isn't sent, unless Replace existing notes is on. Both use the shared interface and the default models from [Preferences ▸ AI Models](app.md#ai-models): the vision model for an item with a picture, otherwise the chat model, or the vision model when no chat model is set.

- **Before the run.** With no provider added or no default vision model, the run doesn't start and a message says which to set. From 30 selected items the confirmation also names the provider and host that receive the items.
- **What is sent.** One request per step per item: the item's name, file type and notes as JSON, the instructions, and its picture. The picture is its thumbnail, or its original when it has no thumbnail and is an image, as a JPEG at most 1024 pixels on a side. Tags, the source URL, folder names, file paths and other items are not sent. An item with no picture goes as text only.
- **In the background.** The window stays usable during a run. A small panel in the bottom right corner, over any page and the viewer, shows the action's name, a bar, "3 of 10" with the time left, the item being asked and Cancel; it also shows a run going in another library, and names that library. A spinner beside the library name and the Dock icon show the progress too, and the run counts as a running task for the switch, close and quit warnings. Items are asked one at a time, and the library lock is not held during a request. Cancel stops the request at once. The panel is hidden while presenting.
- **One at a time.** While a run goes in any open library, or has not reported yet, starting another is refused with a message naming the run and its library. Nothing is queued. Actions with only rule steps, and long operations, aren't held up, and each is its own undo step.
- **Results are checked, then applied together.** Nothing is written while the model is asked. A reply must be JSON fitting the step's schema, with no other fields. Tags must be text, at most 32 of them and 64 characters each; a description at most 4000 characters, neither with control characters. When the run ends, or Cancel stops it, every step of the action is applied to the items that came through, in order, as one undo step named "Run *name*".
- **Items that change during a run.** An item is read again just before it is asked, so the request carries its name and notes as they are then. The steps are applied to each item as it is at the end, and change only the fields they set: a rename, a new rating or a new tag made during the run stays, and auto-tag adds to the tags as they are. Write a description keeps notes written during the run and lists the item among those that kept their notes; with Replace existing notes on, it replaces them. Text being typed in the inspector stays in the field and is saved when the field loses focus. Undo goes back to the item as it was when the run applied. An item moved to the Trash or deleted during the run is skipped, gets none of the steps, and is listed with the reason; if that happens before its turn, it isn't sent.
- **Failures.** An item fails on invalid JSON, a wrong shape, an empty reply, a timeout, a refused request, an unreachable provider or a picture that can't be read. A failed item gets none of the action's steps, including its rule steps, and the run goes on to the next item. A reply cut off at the provider's length limit counts as a failure too.
- **The report.** A run where everything went well ends in the usual notice with Undo. Otherwise an alert says how many items changed, which model ran, each failed item with its reason, each skipped item with its reason, which items kept their notes, and how many Cancel stopped before. Finished items stay applied after Cancel, and ⌘Z reverses them all. An alert that comes while another is up waits for it to close. A notification goes out too, under Preferences ▸ Notification's main switch; macOS shows it only while falcon isn't the active app.
- **Model output is data.** A reply can only become tags or notes (`StepOutput`). The model is offered no tools (on Anthropic, one output tool whose input is the reply), nothing in a reply is run, and the instructions tell the model that the item's text is data, not instructions.

Switching away from the library asks first, as with other tasks, and the run keeps going against its own library. Its report then names the library, in the shown one, with no Undo button in the notice: the undo step is in that library's history, for ⌘Z once it is shown. Closing the library asks first, stops the run, applies and writes the finished items and then closes; the report still appears, without Undo, as the undo step goes with the library. Quitting asks first and stops every run the same way; the report is dropped with the app. If the API's `library/switch` leaves another library in the slot, the run waits up to five seconds for its own to come back and never touches the other, even a copy with the same item ids; if its own doesn't return, the items are reported as failed.

### Entries falcon can't run

Falcon keeps every entry exactly as it read it. An entry that has fields other than `id`, `name`, `shortcut` and `steps`, or steps falcon doesn't know (written by a newer falcon, or edited by hand), is listed greyed in the panel and the menu with the note "This action has steps Falcon can't run". Its shortcut is still reserved, and it can be removed. It can't be run, edited or duplicated. Saving another action leaves these entries byte for byte as they were.

## Storage

Falcon keeps each library's actions in one file in its app data folder:

```
~/Library/Application Support/Falcon/actions/<key>.json
```

The folder is `eframe::storage_dir("Falcon")`, the same app data folder that holds falcon's saved state and its `eagle-backup/` copies. `<key>` names the library. Eagle's `metadata.json` carries no library id, so the key is the 64-bit FNV-1a hash of the library's canonical path (symlinks resolved, no trailing slash) as 16 lowercase hex digits (`falcon_library::actions::library_key`). Reopening the same library finds its actions again; two libraries never share a file. A library that is moved or renamed has a new path, so it starts with no actions; its old file stays in the folder.

Falcon writes the file only after an edit, atomically: a temp file in the same `actions/` folder, then a rename, so a failed write leaves the old file whole and no partial file behind. It is compact, or indented with two spaces when the file it replaces was. If the file can't be read, falcon lists nothing and refuses to write over it. Falcon is the only program that writes the file, so it reads it when a library opens or becomes the active one and keeps it in memory between edits; there is no polling.

Nothing in the Actions code touches the library folder. The tests in `crates/falcon-app/src/actions/storage_tests.rs` open libraries that hold an `actions.config.json` with known bytes and modification time, then create, edit, run and delete actions, switch libraries and run the quit flush. They check that the file's bytes and modification time are unchanged, that no file appeared beside it, that the actions landed in the app data folder, and that two libraries don't see each other's actions.

The file is a JSON array. Each falcon entry:

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
| `autoTag` | none | Adds the tags the default model suggests. Falcon's own |
| `describe` | `replace`: boolean, false when absent | Writes the default model's description to the notes. Falcon's own |

The rule `type` names follow the rule names in Eagle's translations. `clearTag` and `clearFolder` are singular, as there.

A sample, which the tests in `crates/falcon-library/src/actions.rs` read and write back byte for byte:

```json
[{"id":"MA1B2C3D4E5F6","name":"File as reference","shortcut":"Command + Shift + 1","steps":[{"type":"addTags","tags":["reference","to sort"]},{"type":"removeTags","tags":["inbox"]},{"type":"addFolders","folders":["MFOLDER000001"]},{"type":"rating","rating":4}]},{"id":"MA1B2C3D4E5F7","name":"Reset","shortcut":"","steps":[{"type":"clearTag"},{"type":"clearFolder"},{"type":"removeFolders","folders":["MFOLDER000002"]},{"type":"rating","rating":0}]}]
```

## Code

- `crates/falcon-library/src/actions.rs`: the file (`ActionsFile`, which keeps each entry's JSON), its place (`library_key`, `store_path`), the steps (`Step`), and `Library::run_steps`.
- `crates/falcon-app/src/actions/`: `mod.rs` holds `ActionsState` (follows the open library, reads and writes its file in `store_dir()`, finds shortcut conflicts), `app.rs` runs actions and routes the menu and the keys, `panel.rs` draws the panel and `editor.rs` the editor. A run goes through the reducer as `Action::RunSteps`, so it lands in the frame's undo step. A run with AI steps goes on from there to `ai/background.rs`, which keeps it in the workspace, draws its panel and files its result once a frame, and to `ai.rs`, which asks the model on a worker thread and applies the results with `Library::run_steps_with` in a transaction of its own.
- `crates/falcon-app/src/menu.rs`: `ActionEntry`, `action_enabled` and the AppKit Actions menu.
