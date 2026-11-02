# Plugins

Eagle 4.0.0 provides some features through plugins rather than the app itself. Falcon has no plugin functionality for now. This file lists everything plugin-related in Eagle, so it can be built later. Everything here comes from Eagle 4.0.0's shipped code. The rows are counted as online/plugin in [PARITY.md](PARITY.md), not as local gaps.

## The plugin platform

These are what any plugin needs:

| Piece | What it does in Eagle |
| --- | --- |
| Plugin types | Window plugins (their own window), inspector plugins (a panel in the inspector), format plugins (thumbnails and a viewer for extra file types) and background-service plugins |
| Plugin API | The JavaScript API plugins call to read and change the library, items, folders, tags and the window |
| Plugin Center | Browse, search, sort, install, update, disable and uninstall plugins |
| Install from a file | Opening a `.eagleplugin` file installs it |
| Plugins button and menu | The toolbar Plugins button, the P key, and Plugin ▸ recent plugins in the item menu |
| Viewer integration | Pinned plugins and a plugin button in the viewer toolbar, and a middle click that opens the plugin panel |
| Unsupported formats | The cannot-preview page suggests a format plugin from the Plugin Center |
| Plugin shortcuts | Keys assigned to plugins in Preferences ▸ Shortcuts |
| Middle-button setting | Preferences ▸ Controls can set the middle button to run a plugin |
| Developer tools | Create a plugin from a template, import a local project, pack, publish, reload and DevTools |

## First-party plugins Eagle's app integrates

Eagle's app has built-in hooks for these specific plugins, found by their plugin ids:

| Plugin id | Feature | Where it appears in Eagle | Needs a network |
| --- | --- | --- | --- |
| `ai-action` | Actions: saved multi-step edits (add or remove tags and folders, set a rating) run on a selection with one shortcut | The Actions menu, the toolbar lightning button, G, and each action's own shortcut. The app reads `<library>/actions.config.json` for the list | No |
| `combine-images` | Combine Images: merge several selected images into one | Item menu, Edit ▸ Edit Image, ⇧⌘M | No |
| `custom-export` | Export in Format: convert and resize on export | Item menu ▸ Export | No |
| `eagle-plugin-search-by-image` | Reverse image search on Google, Bing, Yandex, TinEye and SauceNAO | Item menu ▸ Search by Image | Yes |
| `ai-search` | AI Search: semantic text search ("black cat") and similar-image search, on the device after a model download | The toolbar AI Search button, the search box's camera icon, Visual Match, the AI Search preferences pane and the `aiSearch/*` API routes | Model download |
| `ai-sdk` | AI Models: shared API keys and model settings for AI plugins (OpenAI, Anthropic, Gemini and others) | The AI Models preferences pane | Yes |
| `mcp-server` | Eagle MCP: a Model Context Protocol server so AI assistants can use the library | The Eagle MCP preferences pane | No (local server) |

Eagle's video player also has an internal component called `eagle-notes` that shows timed comments. It isn't an Eagle plugin, and falcon already has timed video comments.

## How falcon would build them

All but one can be plain local code with no outside service:

| Plugin | Falcon plan |
| --- | --- |
| `combine-images` | Local image processing |
| `custom-export` | Local conversion and resizing (falcon's Export in Format already does this) |
| `ai-action` | Local rule steps (tags, folders, rating). AI steps such as auto-tagging go through AI Models below |
| `mcp-server` | A local server on localhost built on falcon's HTTP API, for assistants on the same Mac |
| `ai-search` | An on-device image and text embedding model with an open licence, run on the GPU through a Rust ML library such as `candle`. Only the one-off model download uses the network |
| `ai-sdk` | API key settings for the same providers Eagle accepts (below) |
| `eagle-plugin-search-by-image` | Not planned: it only works by sending images to outside search engines. Similar-image search in your own library comes from `ai-search` |

### AI Models (`ai-sdk`)

This is one settings pane where you add AI providers once. Every AI feature in falcon then uses them through one shared interface, as Eagle's AI plugins share its `ai-sdk`. It accepts the same eight providers as Eagle's plugin (from Eagle's plugin developer documentation):

| Provider | Id | Kind | Settings |
| --- | --- | --- | --- |
| OpenAI | `openai` | Cloud | API key; base URL optional |
| Anthropic | `anthropic` | Cloud | API key; base URL optional |
| Google Gemini | `google` | Cloud | API key; base URL optional |
| DeepSeek | `deepseek` | Cloud | API key; base URL optional |
| Tongyi Qwen | `tongyi` | Cloud | API key; base URL defaults to DashScope's OpenAI-compatible endpoint |
| Ollama | `ollama` | Local | No key; base URL defaults to `http://localhost:11434/v1` |
| LM Studio | `lmstudio` | Local | No key; base URL defaults to `http://localhost:1234/v1` |
| OpenAI-compatible | `openai-compatible` | Custom | Base URL required; API key optional |

How it would work in falcon:

- **Settings:** Preferences ▸ AI Models lists the providers. Each has a key field, a base URL, a Verify button that checks the key, and a model list fetched from the provider. You can also choose a default chat model and a default image (vision) model, as in Eagle.
- **Model names:** models are named `provider::model`, for example `anthropic::<model id>`, as in Eagle.
- **Key storage:** API keys go in the macOS Keychain, never in the library, falcon's settings file or logs.
- **Shared interface:** one internal module offers generate text, stream text, generate structured output (to a JSON schema), and image input through messages. Each falcon AI feature (auto-tagging and descriptions in Actions, captions, the MCP server) calls this module. None of them handles keys itself.
- **Network:** nothing is sent anywhere until you add a provider. Cloud providers receive only what a feature sends for the items you run it on. Ollama, LM Studio and a local OpenAI-compatible server keep everything on the Mac.
- **Format compatibility:** the provider ids, settings and `provider::model` names match Eagle's, so the setup reads the same in both apps. Falcon keeps its own copy and never touches Eagle's plugin data.

## Community plugins

The Plugin Center also offers community plugins that aren't in Eagle's code. They can't be listed from the app, and they would only matter once the platform above exists.

## Notes on falcon today

- Falcon's Actions were removed, because Eagle's Actions are the `ai-action` plugin. Falcon never reads or writes `actions.config.json`.
- Falcon has its own Export in Format (⇧E): JPG, PNG or WebP, with quality and size. In Eagle this is the `custom-export` plugin, so it overlaps this list.
- Falcon's typo-tolerant search is its own feature, not Eagle's AI Search.
- The HTTP API answers Eagle's routes but not `aiSearch/*`.
