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

The auth check accepts a request whose `Host` header is `localhost:<port>` or `127.0.0.1:<port>`.

v1 handlers reject with a string, which lands in `data`. v2 handlers throw an `Error`; Eagle's JSON server puts its text in `message` and serialises the error object itself as `data`, which comes out as `{}` (observed in the bundled `json-rest-light` module). A v2 handler that returns nothing gives `{"status":"success"}` with no `data`, and `false` gives `"data":false`. Each route keeps Eagle's choice between throwing, `false` and nothing.

## Routes

- **v1:**
  - `application/info`
  - `library/info`, `history`, `switch` and `icon`
  - `folder/list`, `listRecent`, `create`, `rename` and `update`
  - `tag/all`, `list`, `listRecent` and `groups`
  - `item/addFromPath(s)`, `addFromURL(s)`, `update`, `info`, `thumbnail`, `moveToTrash` and `list`
- **v2:** every route in the table below.

v2 lists are paginated as `{data, total, offset, limit}`. `limit` defaults to 50 (also for 0 or junk) and is capped at 1000; `offset` defaults to 0.

Eagle's quirks are kept:
- **Paging:** in `item/list`, `offset` is a page number, and the page is re-sorted newest first after it is cut.
- **Ratings:** v1 `item/update` ignores a rating of 0; v2 `item/update` accepts 0 to 5.

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
| `item/setCustomThumbnail`, `refreshThumbnail` | POST | `itemId` (and `filePath`) | `true` / `false` | not served by falcon (see below) |
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

`library/history` comes from Eagle's app settings (`libraryHistory`). Falcon's server can't reach the app's recent list, so it reports the libraries it has served since it started, newest first, beginning with the one it was started on. Libraries the app hands it with `set_library` count as served.

### The desktop app

Eagle has one library open per window, and its API serves that library; `library/switch` goes through the same code as the library panel (observed in `js/api-server.js`, which calls the window's `openLibrary`). Falcon's app can keep several libraries open (see [Several libraries](app.md#several-libraries)), and the API follows the one being shown:

- The running server keeps its port. When the shown library changes, the app calls `Handle::set_library(Shared)`, which swaps the library the next request sees and adds it to `library/history`. `Handle::library()` returns the one being served. `start` and `serve` are unchanged.
- `library/switch` still replaces the library inside the served slot. The app checks the slot's root every frame. When it changed, the app puts the library that was there back into the slot (reopened from disk; the server's drop of it has already written anything pending) and shows the new path in a workspace of its own: an existing one if that library is already open, otherwise a new one with fresh state and no undo history. The path also joins the app's recent list. If the old library can't be reopened, the shown workspace becomes the new library and starts over.

## How falcon differs, on purpose

- **Local only:** falcon listens on 127.0.0.1 only.
- **Library routes only:** routes that aren't about library data (AI search, plugins, script injection, collect preferences, window control) are left out.
- **Application info:** it reports version and platform, nothing else.
- **Imports:** `addFromPath` and v2 `item/add` reply after the import has run, so a returned id exists. Eagle makes up the id, replies, and imports later. A failed single `item/add` therefore throws in falcon, and a batch lists only the ids that imported.
- **Bookmarks:** `item/add` with `bookmarkURL` throws `bookmarks are not supported`.
- **Thumbnail routes:** `item/setCustomThumbnail` and `item/refreshThumbnail` aren't served yet.
- **Colours:** folder, smart-folder and tag-group colours must be one of Eagle's eight names; anything else throws. `""` or `null` means the default and removes the key. Eagle stores whatever it is given.
- **Tag rename:** `tag/update` also renames the tag in the recent and starred lists, as falcon's own rename does. Eagle's API leaves those lists alone.
- **Tag groups:** names are NFC-normalised and cut to 1024 characters, and tags are de-duplicated, as in falcon's tag manager.
- **Comments:** they are written in the order falcon's model uses (`id`, region or `duration`, `annotation`, `lastModified`). The `addComment` reply uses Eagle's order.
- **Smart folders:** a top-level entry made through the API is stored in the shape Eagle's save gives top-level entries, with no empty `icon` or `iconColor`. A nested one is stored as the API builds it.
- **Library history:** see above.
- **Several libraries:** the app can keep several libraries open; the API serves the one shown, and `library/switch` opens or focuses a workspace instead of replacing the window's only library (see above).

## Ports

Eagle listens on `*:41595`. macOS still lets another program bind `127.0.0.1:41595`, and that program would then receive every localhost request meant for Eagle. Falcon therefore checks whether a port already answers before binding it. The app always starts at 41596, and `falcon serve` refuses to start on a port that is already in use.
