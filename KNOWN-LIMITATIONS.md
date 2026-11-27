# Known limitations (owner build)

Things that are not done, not verified, or behave in a way worth knowing.
Nothing here is hidden elsewhere; each item says how it was found.

## Not verified by an interactive GUI run
These need an operating-system file panel or a drag into another app, which
the automation used for GUI verification cannot drive (it may click Cancel on a
panel but not use it). They have unit/integration tests where noted; the manual
steps are in `OWNER-ACCEPTANCE.md`.
- Choosing the file or folder in the Relink dialog (the dialog itself, its
  Indonesian and Balinese wording and the "nothing matched" list were seen; the
  relink logic is covered by `tests/relink.rs` and `relink` unit tests).
- Move Library from the menu (folder chooser). The transaction is covered by 19
  tests including injected failures, a mixed Copy/Link library moved with the
  sync and thumbnail workers active, and byte-identical external originals.
- Finder Drag Out (drag from the grid into Finder).
- Choosing an Auto-Import folder with the chooser (the Auto-Import itself was
  verified by dropping a file into the watched folder).
- Quitting through the menu bar while driven by accessibility automation logs a
  winit re-entrancy panic ("tried to handle event while another event is
  currently being handled"); state is saved before it. It was not seen with a
  real ⌘Q and is not confirmed as a defect.

## Behaviour and design limits
- A synchronized folder's source cannot be repointed; renaming or moving the
  source folder makes it Offline. Use Relink for moved files, or remove and add
  the folder (Link items already imported are matched by source path, so a new
  source path imports again).
- Throughput of background sync is bounded by the library's durable commit
  (about four fsyncs per file): see the benchmark in `OWNER-DIFFERENCES.md`.
  It is slower in wall-clock than holding the lock for the whole pass, by design.
- Files modified within 2 s of a pass are left for a later pass (retried every
  4 s up to 30 times).
- Without macOS file events (other systems) automatic folders run a pass every 5
  minutes; with them, a pass also runs every 5 minutes as reconciliation.
- Status and mode labels in Preferences ▸ Folder Synchronization ("Synced",
  "Link (Place)", "Mode: Automatic") are English in every language.
- The thumbnail worker and a sync refresh both write
  `.falcon-bg-<id>_thumbnail.png` in an item's folder; they do not normally work
  on the same item at once.
- In the Preferences window a macOS permission prompt for a removable volume can
  hold the app on "Opening library" until it is answered: a rebuilt, ad-hoc
  signed app is asked again for each library on an external drive (seen while
  testing; not a Falcon bug). The QA sandbox therefore lives under
  `~/FalconSandbox`.
- Machine-written translations (including every new Balinese string) are
  unreviewed; see `crates/falcon-app/locales/REVIEW.md`.
- `cargo fmt --check` fails on the upstream baseline; only new files are
  formatted.
