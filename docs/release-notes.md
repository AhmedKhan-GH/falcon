Falcon for Apple Silicon Macs, 64-bit Windows and 64-bit Linux. Falcon is early alpha. The Mac app isn't notarised by Apple, the Windows downloads aren't code-signed, and the Linux packages aren't signed.

**New in 0.1.4:** on Windows, file dialogs can no longer hide behind Falcon's window and leave it looking frozen, as could happen when creating a first library. Renaming a folder selects its whole name, so typing replaces Untitled, and the in-window menus no longer cut off long items.

**Downloads**

| Download | For | Install |
|---|---|---|
| Mac · `Falcon-arm64.dmg` | Macs with Apple Silicon (M1 or newer) and macOS 12 or later. No Intel Macs yet | Open it and drag Falcon to Applications |
| Windows · `Falcon-x86_64-setup.exe` | 64-bit Windows on Intel and AMD. Only Windows 11 has been tried | Run it. It installs for you alone, without admin rights, in `%LOCALAPPDATA%\Programs\Falcon`, with a Start menu entry and an uninstaller in Add or remove programs |
| Windows · `Falcon-x86_64-portable.zip` | The same as the installer | Nothing to install: unzip it anywhere and run `Falcon\falcon-app.exe`. Its DLLs sit in `Falcon\lib`; to remove it, delete the folder |
| Linux · `Falcon-x86_64.AppImage` | Most distributions from 2022 or newer, on 64-bit Intel and AMD | Nothing to install: `chmod +x Falcon-x86_64.AppImage`, then `./Falcon-x86_64.AppImage` |
| Linux · `Falcon-x86_64.deb` | Ubuntu, Debian and distributions based on them, from 2022 or newer | `sudo apt install ./Falcon-x86_64.deb`, which adds Falcon to the app menu |

**First launch on a Mac.** macOS refuses Falcon at first because Apple hasn't checked it. Open System Settings ▸ Privacy & Security, scroll down and click **Open Anyway** next to the message about Falcon. On macOS 14 and earlier you can instead right-click Falcon in Applications and choose **Open**. Or run `xattr -dr com.apple.quarantine /Applications/Falcon.app` in Terminal, which also helps if macOS says Falcon is damaged. 
**First launch on Windows.** Neither Windows download is code-signed, so Windows SmartScreen warns you the first time. Click **More info**, then **Run anyway**. Uninstalling leaves your libraries and settings alone.

On Linux, both downloads open straight away.

To update, download the new version and install it the same way. Falcon doesn't update itself yet.

**While Falcon is in development, it only opens libraries inside `~/Falcon Sandbox`,** so copy a library there to try it. On Windows that is `%USERPROFILE%\Falcon Sandbox`.

**Works on Linux:** libraries, import, thumbnails, tags, folders, search, the image viewer, video playback through a bundled FFmpeg, the local API and the menu bar.

**Works on Windows:** what works on Linux, including video through the bundled FFmpeg. An item whose name Windows can't keep, such as `a:b`, is kept on disk as `a_b.png`, so it won't find its file if the library goes back to a Mac.

**Not on Windows yet:** everything not on Linux yet (below), and also screen capture, global hotkeys, the share sheet, Touch ID, starting at login, transparency effects, iCloud handling, opening files from File Explorer and links that open Falcon. Open With shows Windows' own window, and revealing several files shows only the first in each folder. Cloud AI providers can't be set up, since there is nowhere to keep an API key; local models such as Ollama don't need one.

**Not on Linux yet**

- The built-in PDF reader: PDFs open in your default app, with no thumbnail
- Live web pages and bookmark screenshots
- Screen capture
- HEIC, AVIF animation and JPEG XL viewing
- Thumbnails for formats that rely on macOS QuickLook, such as PowerPoint
- Dragging items out of the window, the tray icon and notifications
- Cloud AI providers, since there is nowhere to keep an API key; local models such as Ollama don't need one

To build Falcon yourself, see the [README](https://github.com/AhmedKhan-GH/falcon#get-started).
