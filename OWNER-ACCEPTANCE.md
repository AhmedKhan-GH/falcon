# Owner acceptance checklist

Use a throwaway library, never the real one. Build the QA sandbox with
`FALCON_QA_DIR=~/FalconSandbox/qa-N cargo test -p falcon-library --test qa_fixture -- --ignored`
and start the app with the same variable (QA mode: data in `falcon-qa`, only
paths under that folder open).

Verified by GUI automation with screenshots (see `FALCON_QA/gui-evidence`):
- [x] Grid zoom slider drag does not move the window
- [x] Preview zoom slider drag does not move the window
- [x] Preview at 1:1 shows the full-resolution 4096×3072 source (1 px grid crisp)
- [x] Choosing a sidebar folder in Preview Mode returns to the grid
- [x] Path Bar breadcrumbs are clickable; the setting survives a restart
- [x] Hide Unsupported Files hides 3 files without removing them (44 vs 41)
- [x] Auto-Import brought a dropped file in
- [x] Background Folder Sync: 30 files added at launch with the window usable;
      a changed source refreshed its dimensions and thumbnail by itself
- [x] Missing media badge and the Relink dialog (Indonesian and Balinese)
- [x] Indonesian Inspector: "Tanggal Dimodifikasi" no longer overlaps or cuts
- [x] All eight Balinese terms in the live interface: Ten Mekategori, Ten
      Metag, Wau Keanggen, Wadah Luu (sidebar/menu), Buka ring Aplikasi
      (Inspector), Link Malih (dialog), Engkebang berkas sane nenten kedukung
      (menu), buwung (dialog button)

To do by hand (needs a system file panel or a drag):
- [ ] Relink Original… → pick `sources/relink-new/ref-01.png`, then Relink; the
      badge goes away and the original file is unchanged
- [ ] Library ▸ Move Library… to another folder; the library opens from the
      new place, the old folder is in the Trash, linked originals still open
- [ ] Drag an item from the grid into a Finder window (a copy of the original
      appears; a linked item gives the original's file)
- [ ] Preferences ▸ Auto-Import ▸ Choose Folder… inside the sandbox
- [ ] Press ⌘Q (not the menu item) and confirm a clean exit with no crash report
- [ ] Narrow the Inspector to its minimum in Indonesian, Balinese, German and
      Japanese: every row stays readable
