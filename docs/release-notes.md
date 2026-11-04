The first downloadable build of Falcon, for 64-bit Linux. Falcon is early alpha and unsigned.

**Install**

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

macOS users can still build from source: see the [README](https://github.com/AhmedKhan-GH/falcon#get-started).
