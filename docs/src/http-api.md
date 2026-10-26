# HTTP API

Eagle serves a local JSON API on port 41595 while a library is open. Extensions, scripts and MCP servers use it. Falcon serves the same API from `falcon-server`, both inside the desktop app and through `falcon serve <library>`.

## Envelope (observed)

| Outcome | HTTP status | Body |
|---|---|---|
| success | 200 | `{"status":"success","data":…}`, or no `data` at all |
| string error | 500 | `{"status":"error","data":"File does not exist."}` |
| unknown path | 404 | `"message":"method not allowed"` (Eagle's own wording) |
| wrong method | 405 | `"message":"method not allowed"` |

The auth check accepts a request whose `Host` header is `localhost:<port>` or `127.0.0.1:<port>`.

## Routes

- **v1:**
  - `application/info`
  - `library/info` and `history`
  - `folder/list`, `listRecent`, `create`, `rename` and `update`
  - `tag/all`, `list`, `listRecent` and `groups`
  - `item/addFromPath(s)`, `addFromURL(s)`, `update`, `info`, `thumbnail`, `moveToTrash` and `list`
- **v2:**
  - `app/info`, `library/info`
  - `item/get`, `query` and `countAll`
  - `folder/get`, `tag/get`

v2 lists are paginated as `{data, total, offset, limit}`.

Eagle's quirks are kept:
- **Paging:** in `item/list`, `offset` is a page number, and the page is re-sorted newest first after it is cut.
- **Ratings:** `item/update` ignores a rating of 0.

## How falcon differs, on purpose

- **Local only:** falcon listens on 127.0.0.1 only.
- **Library routes only:** routes that aren't about libraries, items, folders or tags are left out.
- **Application info:** it reports version and platform, nothing else.
- **Imports:** `addFromPath` replies after the import has run, so the returned id exists. Eagle replies before.

## Ports

Eagle listens on `*:41595`. macOS still lets another program bind `127.0.0.1:41595`, and that program would then receive every localhost request meant for Eagle. Falcon therefore checks whether a port already answers before binding it. The app always starts at 41596, and `falcon serve` refuses to start on a port that is already in use.
