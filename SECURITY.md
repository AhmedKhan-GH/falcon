# Security

## Reporting a vulnerability

Please report security issues privately through GitHub's [private vulnerability reporting](https://github.com/AhmedKhan-GH/falcon/security/advisories/new) rather than in a public issue. Include the steps to reproduce and the version or commit you tested.

## Scope

Falcon reads and writes files in the libraries you open, and while the app runs it serves a local HTTP API.

- The API listens on `127.0.0.1` only and accepts requests whose `Host` header is `localhost` or `127.0.0.1` on its own port.
- The API never runs code it receives, and never fetches remote scripts.
- The MCP server is off by default. When on, it listens on `127.0.0.1` only, refuses requests with a foreign `Host` or `Origin`, and can require a bearer token. Without one, any program on the Mac can read the open library. Write tools are a separate setting, also off by default, and none deletes anything for good.
- Imports read only the files you choose, plus URLs you pass to the `addFromURL` route.
