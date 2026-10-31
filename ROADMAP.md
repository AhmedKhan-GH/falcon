# Roadmap

Falcon is early alpha. This is what is done and what comes next, roughly in order.

## Done

- **Library format:** reads and writes Eagle-format libraries, byte-identical on round trips.
- **Import:** thumbnails and colour palettes, QuickLook formats (PDF, video, PSD, AI, HEIC, RAW), background imports, paste, drag and drop.
- **Organise:** folders, tags, smart folders, Quick Access, ratings, notes, trash, batch editing.
- **Search:** keyword syntax, typo tolerance, the filter bar, colour similarity search, sorting.
- **Browse:** Waterfall, Justified, Grid and List layouts, the viewer, dragging items out to other apps.
- **Local HTTP API and CLI.**

## Next

- [x] Hover preview in the grid
- [x] Per-folder sort, colours and icons
- [ ] Video and audio playback in the viewer
- [x] Tag groups
- [x] Undo
- [ ] Windows and Linux builds
- [ ] Signed, notarised macOS releases

## Deferred local features

Eagle features that need no online service, put off until after the list above.

**Browsing and look**
- [x] Reorder folders by drag
- [ ] Light, gray, blue and purple themes
- [ ] Window buttons at Eagle's height

**Search and organising**
- [ ] Date, duration, camera (EXIF) and font filters
- [x] Saved filters
- [x] Nested smart folders and smart folder icons

**Editing**
- [ ] Custom thumbnails
- [ ] Refresh a palette
- [ ] Folder covers
- [x] Folder passwords

**Viewer**
- [ ] Comments on a region of an image
- [ ] Crop and rotate

**File formats**
- [ ] Audio waveforms
- [ ] Font previews
- [ ] 3D model thumbnails

**Import**
- [ ] Screenshots
- [ ] IPTC and XMP keywords

**Library**
- [x] Batched writes and backup rotation
- [ ] Duplicate detection
- [x] Export
- [ ] Several libraries open at once
- [ ] Sync conflict detection for libraries in iCloud Drive or Dropbox

**App**
- [x] Preferences
- [x] Shortcut settings
- [x] Collect window
- [ ] Localisation

**HTTP API**
- [x] Remaining v2 routes: comments, tag groups, smart folders, updates, library switching

## Later

- [ ] Browser extension for saving images
- [ ] Plugin system
- [ ] AI search (a plugin in Eagle)
