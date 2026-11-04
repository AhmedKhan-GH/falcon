Falcon for Apple Silicon Macs and 64-bit Linux. Falcon is early alpha. The Mac app isn't notarised by Apple, and the Linux packages aren't signed.

**Install on a Mac**

For Macs with Apple Silicon (M1 or newer) and macOS 12 or later. There is no download for Intel Macs yet.

1. Open `Falcon-arm64.dmg` and drag Falcon to Applications.
2. The first time you open Falcon, macOS refuses because Apple hasn't checked it. Open System Settings ▸ Privacy & Security, scroll down and click **Open Anyway** next to the message about Falcon. On macOS 14 and earlier you can instead right-click Falcon in Applications and choose **Open**. Or run `xattr -dr com.apple.quarantine /Applications/Falcon.app` in Terminal, which also helps if macOS says Falcon is damaged.

**Install on Linux**

- **AppImage, for most distributions:** `chmod +x Falcon-x86_64.AppImage`, then `./Falcon-x86_64.AppImage`
- **.deb, for Ubuntu and Debian:** `sudo apt install ./falcon_amd64.deb`

To update, download the new version and install it the same way. Falcon doesn't update itself yet.

**While Falcon is in development, it only opens libraries inside `~/Falcon Sandbox`,** so copy a library there to try it.

**Works on Linux:** libraries, import, thumbnails, tags, folders, search, the image viewer, video playback through a bundled FFmpeg, the local API and the menu bar.

**Not on Linux yet**

- The built-in PDF reader: PDFs open in your default app, with no thumbnail
- Live web pages and bookmark screenshots
- Screen capture
- HEIC, AVIF animation and JPEG XL viewing
- Thumbnails for formats that rely on macOS QuickLook, such as PowerPoint
- Dragging items out of the window, the tray icon and notifications

To build Falcon yourself, see the [README](https://github.com/AhmedKhan-GH/falcon#get-started).
