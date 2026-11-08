# HTTP API

Eagle serves a local JSON API on port 41595 while a library is open. Extensions, scripts and MCP servers use it. Falcon serves the same API from `falcon-server`, both inside the desktop app and through `falcon serve <library>`.

## Envelope (observed)

| Outcome | HTTP status | Body |
|---|---|---|
| success | 200 | `{"status":"success","data":…}`, or no `data` at all |
| string error (v1) | 500 | `{"status":"error","data":"File does not exist."}` |
| thrown error (v2) | 500 | `{"status":"error","message":"id is required","data":{}}` |
| unknown path | 404 | `"message":"method not allowed"` (Eagle's own wording) |
| wrong method | 405 | `"message":"method not allowed"` |

A request is served when its `Host` header is `localhost:<port>`, `127.0.0.1:<port>` or `0.0.0.0:<port>`, or when it carries the Developer token as `token`: in the query string for GET, in the JSON body or the query string for POST. With no token set, which is the default, only the Host check applies, and no `token` value (not even an empty one) lets a request from another Host in. With a token set, the value must match exactly. A token that is only whitespace counts as no token. Anything else gets 401. Eagle does the same (observed in `json-rest-light`), with its port hard-coded to 41595. Eagle's `/item`, `/folder`, `/smart-folder` and playground handlers skip the check entirely; falcon applies it to them too, on purpose.

v1 handlers reject with a string, which lands in `data`. v2 handlers throw an `Error`; Eagle's JSON server puts its text in `message` and serialises the error object itself as `data`, which comes out as `{}` (observed in the bundled `json-rest-light` module). A v2 handler that returns nothing gives `{"status":"success"}` with no `data`, and `false` gives `"data":false`. Each route keeps Eagle's choice between throwing, `false` and nothing.

## Routes

- **v1:**
  - `application/info` (also served at `/`)
  - `library/info`, `history`, `switch` and `icon`
  - `folder/list`, `listRecent`, `create`, `rename`, `update` and `unlock`
  - `preferences/collect/on` and `off`
  - `tag/all`, `list`, `listRecent` and `groups`
  - `item/addFromPath(s)`, `addFromURL(s)`, `update`, `info`, `thumbnail`, `setCustomThumbnail`, `refreshPalette`, `refreshThumbnail`, `moveToTrash` and `list`
- **Links:** `/item`, `/folder` and `/smart-folder` redirect to `eagle://` links (see "Link redirects").
- **Playground:** `GET /api/v2/playground` (see "The playground").
- **v2:** every route in the table below.

v2 lists are paginated as `{data, total, offset, limit}`. `limit` defaults to 50 (also for 0 or junk) and is capped at 1000; `offset` defaults to 0.

Eagle's quirks are kept:
- **Paging:** in `item/list`, `offset` is a page number, and the page is re-sorted newest first after it is cut.
- **Ratings:** v1 `item/update` ignores a rating of 0; v2 `item/update` accepts 0 to 5.

## More v1 routes

| Route | Method | Parameters | Reply | Errors |
|---|---|---|---|---|
| `application/info`, `/` | GET | | `{version, prereleaseVersion, buildVersion, showCollectModal, platform, preferences}`. `showCollectModal` is a boolean, true when `preferences.general.showCollectModal` is `"true"`. `preferences` is the app's Eagle-shaped subset; `falcon serve` reports `{"general":{"showCollectModal":"false"},"developer":{"apiToken":""}}` | |
| `folder/unlock` | POST | `folderId`, `password` | success without data | `Folder does not exist.`, `Password is incorrect.` |
| `preferences/collect/on`, `off` | GET | | success without data; sets `general.showCollectModal` to `"true"` or `"false"` | `preferences file not exist.` |
| `item/setCustomThumbnail` | POST | `id`, `thumbnailPath` | `true` | `File does not exist.`, `Failed to set custom thumbnail.` |
| `item/refreshPalette` | POST | `id` | success without data | `File does not exist.` |
| `item/refreshThumbnail` | POST | `id` | success without data | `File does not exist.` |

`folder/unlock` lasts for the server's session only. Until it ends, `folder/list` and `folder/listRecent` report that folder with `isUnLock: true` and its children. Nothing is written to the library.

## Link redirects

`/item?id=<id>`, `/folder?id=<id>` and `/smart-folder?id=<id>` answer 302 with a `Location` of `eagle://item/<id>`, `eagle://folder/<id>` or `eagle://smart-folder/<id>`. An unknown id answers 404 with `{"error":"File does not exist."}`, `{"error":"Folder does not exist."}` or `{"error":"Smart Folder does not exist."}`. These paths go through the same auth check as the rest.

## The playground

`GET /api/v2/playground` serves falcon's own page, not a copy of Eagle's. It lists the routes, has a form for the parameters of each, a field for the token, and shows the equivalent `curl` line.

## eagle:// links

The app bundle registers the `eagle` scheme (`CFBundleURLTypes` in `scripts/bundle.sh`). The app handles `eagle://item/<id>`, `eagle://folder/<id>` and `eagle://smart-folder/<id>`, both from clicked links and as launch arguments. A clicked link reaches the running app as an Apple "get URL" event, which falcon catches before the event loop starts. An item opens in the viewer inside its first folder, or All when it has none; a folder or smart folder is shown. Ids the shown library doesn't have are ignored, and links wait while the app is locked. `eagle://openFolder/<id>`, the form Eagle's "added to folder" notices link with, opens the folder in the app as a folder link does (Eagle sends it to the same handler). Plugin links are not handled.

The bundle also registers image, video, design, `.eaglepack` and `.library` document types, so Finder's Open With and Dock drops reach falcon. Files are imported into the shown library, a package is imported as a package, and a library is opened. Folders can be dropped on the Dock icon, but falcon does not become any folder's default app.

The files reach the running app as an Apple "open documents" event, handled in `crates/falcon-app/src/open_files.rs` and passed to the same import path as a drop (`App::import`), as Eagle's `open-file` handler imports files and opens packages. AppKit installs its own handler for that event while the app finishes launching, so falcon installs its handler when AppKit posts `NSApplicationWillFinishLaunchingNotification`, which is still before a launch's own files arrive. A package asks first, showing its size, as Eagle does. With no library open, files other than a library are ignored.

## v2 routes (observed)

Read from Eagle 4.0's `app/js/api-server-v2.js` (the route table), `api-v2-playground-config.js` (parameters) and the handlers in `app/js/plugin/index.js` and `plugin/handlers/smart-folder-handlers.js`. "Paged" means the reply is wrapped as above. GET takes the query string, POST a JSON body, so array parameters only work through POST.

| Route | Method | Parameters | Reply | Errors and empty replies |
|---|---|---|---|---|
| `app/info` | GET | | `{version, prereleaseVersion, buildVersion, platform}` | |
| `library/info` | GET | | library `metadata.json` plus top-level `path` and `name` (v1 nests them under `library`) | |
| `library/history` | GET | `offset`, `limit` | paged list of library paths | |
| `library/switch` | POST | `libraryPath` | `true` | `Library does not exist.` |
| `library/icon` | GET | `libraryPath` | `icon.png` as Node prints a Buffer: `{"type":"Buffer","data":[…bytes]}` | `Library does not exist.`, `Library icon does not exist.` |
| `item/get` | GET, POST | `id`, `ids`, `keywords`, `tags`, `folders`, `smartFolders`, `ext`, `annotation`, `url`, `rating`, `shape`, `isUnfiled`, `isUntagged`, `isSelected`, `fields`, `offset`, `limit` | paged items | `smart folder not found: <id>` |
| `item/query` | POST | `query`, `offset`, `limit` | paged items | |
| `item/countAll` | GET | | number of items, trash included | |
| `item/add` | POST | one of `path`, `url`, `base64`, `bookmarkURL`; `name`, `website`, `tags`, `annotation`, `folders`; or `items` (up to 1000 such entries, which may also carry `star` and `modificationTime`) | `{id}`, or `{ids}` for `items` | `item.add requires at least one of: url, path, base64, bookmarkURL`, `Batch limit exceeded: max 1000 items per request, got <n>`, `not support for directory` |
| `item/update` | POST | `id`; any of `name`, `annotation`, `url`, `tags`, `folders`, `star` (0–5), `ext`, `width`, `height`, `noThumbnail`, `noPreview`, `isDeleted`, `modificationTime` | the item | `false` for an unknown item; `name too long (max 10000)`, `annotation too long (max 10000)` |
| `item/getComments` | GET | `id` | the item's `comments` array, `[]` when none (not paged) | `id is required`, `item not found: <id>` |
| `item/addComment` | POST | `id`, `annotation`, and either `x`, `y`, `width`, `height` (rounded) or `duration` | the comment: `id`, `annotation`, `lastModified`, then the region or `duration` | `cannot have both duration and x/y/width/height`, `must provide either duration (video) or x/y/width/height (image)`, `duration must be >= 0`, `x, y, width, height must all be numbers`, `width and height must be > 0`, `annotation exceeds max length (10000)` |
| `item/updateComment` | POST | `id`, `commentId`, then the same fields as add | the comment, with `lastModified` set to now | `commentId is required`, `no comments found`, `comment not found: <id>`, `cannot set duration on an image comment`, `cannot set x/y/width/height on a video comment`, `width must be > 0`, `height must be > 0` |
| `item/removeComment` | POST | `id`, `commentId` | `true` | as update |
| `item/setCustomThumbnail` | POST | `itemId`, `filePath` | `true`, or `false` when the item is unknown or the file can't be used | never throws |
| `item/refreshThumbnail` | POST | `itemId` | `true` for a known item, `false` otherwise | never throws |
| `folder/get` | GET, POST | `id`, `ids`, `isRecent`, `isSelected`, `offset`, `limit` | paged folders: the top-level tree with no filter, a flat list with one | |
| `folder/create` | POST | `name`, `description`, `parent`, `iconColor` | the folder | `false` without a name; an unknown `parent` puts it at the root; `name too long (max 10000)` |
| `folder/update` | POST | `id`, `name`, `description`, `tags`, `iconColor`, `parent` | the folder | nothing for an unknown folder; `name too long …`, `description too long …` |
| `smartFolder/get` | GET, POST | `id`, `ids` (POST), `offset`, `limit` | paged: the whole tree with `imageCount` on each entry, or a flat list for `id`/`ids` | |
| `smartFolder/create` | POST | `name`, `conditions`, `description`, `icon`, `iconColor`, `parent` | `{id, name, conditions, description, icon, iconColor, children, modificationTime, imageCount}` | `name is required`, `name too long (max 10000)`, `conditions is required`, the rule errors below, `description too long (max 10000)`, `parent smart folder not found: <id>` |
| `smartFolder/update` | POST | `id`, `name`, `conditions`, `description`, `icon`, `iconColor` | the entry with `imageCount`; `modificationTime` always stamped | `id is required`, `smart folder not found: <id>`, `name cannot be empty`, rule errors |
| `smartFolder/remove` | POST | `id` | `true`; children and their Quick Access pins go too | `id is required`, `smart folder not found: <id>` |
| `smartFolder/getItems` | GET, POST | `smartFolderId`, `orderBy`, `fields`, `offset`, `limit` | paged live items; `fields` keeps only those keys, in the order asked | `smartFolderId is required`, `smart folder not found: <id>` |
| `smartFolder/getRules` | GET | | the rule schema: per property its `category`, `methods`, `valueType`, and when present `noValueMethods`, `options`, `extraFields` | |
| `tag/get` | GET, POST | `name` (case-insensitive substring), `offset`, `limit` | paged tags | |
| `tag/getRecentTags`, `getStarredTags` | GET | `offset`, `limit` | paged tags from `historyTags` / `starredTags` that are still in use | |
| `tag/update` | POST | `originalName`, `name` | the renamed tag | nothing for a missing or unknown `originalName`; `name too long (max 10000)` |
| `tag/merge` | POST | `source`, `target` | `{affectedItems, sourceRemoved: true}` | `tag.merge requires "source" and "target"`, `tag.merge: source and target must be different`; nothing when either tag is unknown |
| `tagGroup/get` | GET | `offset`, `limit` | paged groups | |
| `tagGroup/create` | POST | `name`, `tags`, `color`, `description` | the group (`id`, `name`, `tags`, then `color` and `description` when given) | `false` without a name; `name too long (max 10000)` |
| `tagGroup/update` | POST | `id`, `name`, `tags` (replaces), `color`, `description` | the group | nothing without `id` or for an unknown group |
| `tagGroup/remove` | POST | `id` | `true`; the tags stay on their items | `false` for an unknown group |
| `tagGroup/addTags` | POST | `groupId`, `tags`, `removeFromSource` | the group | `tagGroup.addTags requires "tags" array`, `tags array too large (max 1000)`; nothing for an unknown group |
| `tagGroup/removeTags` | POST | `groupId`, `tags` | the group | `tagGroup.removeTags requires "tags" array`, `tags array too large (max 1000)`; nothing for an unknown group |

`tagGroup/addTags` keeps a tag in its other groups unless `removeFromSource` is set, in which case it moves.

Smart-folder `conditions` are checked strictly before anything is saved. The first problem is reported, in Eagle's words: `conditions must be a non-empty array`, `max 30 conditions`, `each condition must have non-empty rules array`, `max 30 rules per condition`, `match must be "AND" or "OR"`, `boolean must be "TRUE" or "FALSE"`, then per rule `invalid property: …`, `invalid method '…' for property '…'. Valid methods: …`, `value is required …`, a type message for the value, and the `unit` or custom-shape `width`/`height` requirements.

**Updates:** Eagle 4.0 has no change feed, cursor or "changed since" route in either API version. The update routes are `item/update`, `folder/update`, `tag/update`, `tagGroup/update` and `smartFolder/update`, all served. A client that wants to notice changes has to poll, for example `library/info`'s `modificationTime` (bumped on every library `metadata.json` write) or the items' `lastModified` (inferred; Eagle documents no polling scheme).

**Folder update quirk (observed):** `folder/update` compares `iconColor` and `parent` with the folder's current values even when they are left out. A request without `iconColor` clears the colour, and one without `parent` moves a subfolder to the root. Falcon does the same, so clients should always send both. A move into the folder itself or its own subtree is skipped silently.

## Library switching

`library/switch` (v1 and v2) opens the given path into the server's library slot, which the desktop app shares, so the app shows the new library too (see "The desktop app" below). Falcon refuses a path that does not exist (`Library does not exist.`), and also one that is not a folder ending in `.library` with a `metadata.json` (`Not an Eagle library.`, falcon's own text: Eagle tries to open anything that exists). Opening only reads, so nothing is created or written in the target.

`library/history` comes from Eagle's app settings (`libraryHistory`). In the desktop app, the app hands its recent libraries list to the server with `Handle::set_history` when the server starts, when the shown library changes and when preferences are applied. `falcon serve` has no such list, so it reports the libraries it has served since it started, newest first, beginning with the one it was started on. Libraries handed over with `set_library` count as served.

### The desktop app

Eagle has one library open per window, and its API serves that library; `library/switch` goes through the same code as the library panel (observed in `js/api-server.js`, which calls the window's `openLibrary`). Falcon's app can keep several libraries open (see [Several libraries](app.md#several-libraries)), and the API follows the one being shown:

- The running server keeps its port. When the shown library changes, the app calls `Handle::set_library(Shared)`, which swaps the library the next request sees and adds it to `library/history`. `Handle::library()` returns the one being served. `start` and `serve` are unchanged.
- `library/switch` still replaces the library inside the served slot. The app checks the slot's root every frame. When it changed, the app puts the library that was there back into the slot (reopened from disk; the server's drop of it has already written anything pending) and shows the new path in a workspace of its own: an existing one if that library is already open, otherwise a new one with fresh state and no undo history. While tasks run in the shown library the app asks first, with Force Switch and Cancel; on Cancel the served library stays the shown one, although the request already answered `true`. The path also joins the app's recent list. If the old library can't be reopened, the shown workspace becomes the new library and starts over.
- `Handle::set_history(paths)` replaces the list `library/history` reports, newest first, dropping duplicates.
- `Handle::set_token(token)` sets the Developer token; an empty token (or one that is only whitespace) turns the token check off: only the Host check applies, as in Eagle. `Handle::set_preferences(value)` sets the preferences `application/info` reports. `Handle::take_preferences()` returns them when a request changed them since the last call (`preferences/collect/on` and `off`), otherwise `None`. The app calls `set_token`, `set_preferences` and `set_history` together, and polls `take_preferences` each frame. It keeps `showCollectModal` changes made over the API; Preferences has no checkbox for it yet.

### Developer token

The Developer pane in Preferences holds the API token (`Prefs::api_token`). It is empty by default, as Eagle's `developer.apiToken` is (observed); falcon doesn't make one on first launch. A token made by an earlier falcon version is kept. While it is empty the pane says "No token. Only requests addressed to localhost are served." and offers a Generate button, which makes a random UUID, Eagle's format. With a token set the pane shows it with Copy, which puts it on the clipboard, Regenerate, which makes a new one, and a link of the form `http://localhost:<first port>/?token=…`. Changes apply on Save or Apply.

The server rule is tested in `falcon-server` (a unit test of the rule and an end-to-end test), and the default and the keeping of an earlier token in the preferences tests.

## How falcon differs, on purpose

- **Local only:** falcon listens on 127.0.0.1 only.
- **Library routes and AI Search only:** routes that aren't about library data (plugins, script injection, window control) are left out, as are bookmarks and batch save.
- **AI Search:** `/api/v2/aiSearch/*` answers from falcon's own AI Search (see the App chapter). `isInstalled`, `isReady`, `isStarting` and `isSyncing` answer a boolean; `getSyncStatus` answers `isSyncing`, `done`, `total` and `pending`; `checkServiceHealth` answers `healthy`, `status` (`notInstalled`, `starting`, `syncing`, `ready` or `error`) and `error`. `searchByText` takes `query` or `text`, `searchByBase64` takes `base64` or `image`, `searchByItemId` takes `itemId` or `id`, and each takes `limit`. They answer the v2 list shape with a `score` on each item. Before the model is installed or ready they throw and say so, and they do under the `falcon` command line, which has no model. These shapes are falcon's reading of the route names; Eagle's own were not recorded.
- **Auth on link and playground paths:** falcon checks Host or token on `/item`, `/folder`, `/smart-folder` and the playground; Eagle doesn't.
- **Imports:** `addFromPath` and v2 `item/add` reply after the import has run, so a returned id exists. Eagle makes up the id, replies, and imports later. A failed single `item/add` therefore throws in falcon, and a batch lists only the ids that imported.
- **Import locking:** the add routes (`addFromPath(s)`, `addFromURL(s)`, v2 `item/add`) read, thumbnail and analyse each source before taking the library lock, and lock only briefly to look up folders and to commit, as the app's own import does. Downloads for `addFromURL(s)` also run unlocked. A slow or dataless iCloud source therefore stalls only its own request; the app, which shares the library, keeps drawing.
- **Bookmarks:** `item/add` with `bookmarkURL` throws `bookmarks are not supported`.
- **Add routes, tags:** URL routes (`addFromURL(s)`, and v2 `item/add` with `url` or `base64`) put the target folders' auto-tags first, then the given tags. Path routes add no auto-tags (observed: Eagle's `uploadUrls` adds `extendTags`, while `uploadFiles` from the API gets no folder). v1 `addFromURL(s)` also move the given tags to the front of the recent tags, as Eagle's `addHistoryTags` does; the path routes and v2 don't.
- **Duplicates:** Eagle imports API items and then, with the duplicate notification on, asks in its window. Falcon imports them without asking, because API imports have no dialog yet. This holds for path and URL routes alike.
- **Thumbnail routes:** `item/setCustomThumbnail`, `refreshPalette` and `refreshThumbnail` finish their work before replying; Eagle queues the work and replies at once. Falcon does it without holding the library lock and writes back only the keys it changed, so edits made meanwhile survive. `library/switch` and `library/icon` also read without holding the lock.
- **Colours:** folder, smart-folder and tag-group colours must be one of Eagle's eight names; anything else throws. `""` or `null` means the default and removes the key. Eagle stores whatever it is given.
- **Tag rename:** `tag/update` also renames the tag in the recent and starred lists, as falcon's own rename does. Eagle's API leaves those lists alone.
- **Tag groups:** names are NFC-normalised and cut to 1024 characters, and tags are de-duplicated, as in falcon's tag manager.
- **Comments:** they are written in the order falcon's model uses (`id`, region or `duration`, `annotation`, `lastModified`). The `addComment` reply uses Eagle's order.
- **Smart folders:** a top-level entry made through the API is stored in the shape Eagle's save gives top-level entries, with no empty `icon` or `iconColor`. A nested one is stored as the API builds it.
- **Library history:** see above.
- **Several libraries:** the app can keep several libraries open; the API serves the one shown, and `library/switch` opens or focuses a workspace instead of replacing the window's only library (see above).

## Ports

Eagle listens on `*:41595`. macOS still lets another program bind `127.0.0.1:41595`, and that program would then receive every localhost request meant for Eagle. Falcon therefore checks whether a port already answers before binding it. The app always starts at 41596, and `falcon serve` refuses to start on a port that is already in use.
