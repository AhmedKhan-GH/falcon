//! Copying one picture as pixels and as a file (Eagle's ⌘C on a single
//! jpg, png or bmp): the general pasteboard gets the image data, the
//! original's file URL and the file name list at once, so an image editor
//! pastes pixels and Finder pastes the file.

use std::path::Path;

/// The pasteboard type that carries a picture's own bytes, by extension
/// (case-insensitive). A bmp goes in as its TIFF only. `None` for formats
/// that are copied as a file alone.
pub fn pixel_type(ext: &str) -> Option<&'static str> {
    match ext.to_ascii_lowercase().as_str() {
        "png" => Some("public.png"),
        "jpg" | "jpeg" => Some("public.jpeg"),
        "bmp" => Some("public.tiff"),
        _ => None,
    }
}

/// Puts the picture at `path` on the general pasteboard as image data, a
/// file URL and a file name list. Reads the file, so not for an original
/// still in iCloud.
#[cfg(target_os = "macos")]
pub fn copy(path: &Path) -> Result<(), String> {
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2::msg_send;
    use objc2_foundation::{NSArray, NSData, NSString, NSURL};

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let kind = pixel_type(ext).ok_or("not a picture format")?;
    let text = path.to_str().ok_or("path is not UTF-8")?;
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let ns_path = NSString::from_str(text);
    let url = NSURL::fileURLWithPath(&ns_path);
    let url_text = url.absoluteString().ok_or("no file URL")?;
    let names = NSArray::from_retained_slice(std::slice::from_ref(&ns_path));
    let own = NSData::with_bytes(&bytes);

    // SAFETY: plain NSPasteboard, NSImage and NSData calls with matching
    // argument and return types; every object is retained for the call.
    unsafe {
        let class = AnyClass::get(c"NSPasteboard").ok_or("no pasteboard")?;
        let board: Option<Retained<AnyObject>> = msg_send![class, generalPasteboard];
        let board = board.ok_or("no pasteboard")?;
        let image_class = AnyClass::get(c"NSImage").ok_or("no NSImage")?;
        let alloc: Allocated<AnyObject> = msg_send![image_class, alloc];
        let image: Option<Retained<AnyObject>> = msg_send![alloc, initWithContentsOfFile: &*ns_path];
        let image = image.ok_or("the picture could not be read")?;
        let tiff: Option<Retained<NSData>> = msg_send![&*image, TIFFRepresentation];
        let tiff = tiff.ok_or("the picture could not be converted")?;

        let _: isize = msg_send![&*board, clearContents];
        let mut ok = true;
        // A bmp has no pasteboard type of its own; its TIFF stands in.
        if kind != "public.tiff" {
            let t = NSString::from_str(kind);
            let done: bool = msg_send![&*board, setData: &*own, forType: &*t];
            ok &= done;
        }
        let t = NSString::from_str("public.tiff");
        let done: bool = msg_send![&*board, setData: &*tiff, forType: &*t];
        ok &= done;
        let t = NSString::from_str("public.file-url");
        let done: bool = msg_send![&*board, setString: &*url_text, forType: &*t];
        ok &= done;
        let t = NSString::from_str("NSFilenamesPboardType");
        let done: bool = msg_send![&*board, setPropertyList: &*names, forType: &*t];
        ok &= done;
        if ok { Ok(()) } else { Err("the pasteboard refused a type".into()) }
    }
}

/// Off macOS there is no such pasteboard; the caller copies the file alone.
#[cfg(not(target_os = "macos"))]
pub fn copy(_path: &Path) -> Result<(), String> {
    Err("not available on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_by_extension() {
        assert_eq!(pixel_type("png"), Some("public.png"));
        assert_eq!(pixel_type("PNG"), Some("public.png"));
        assert_eq!(pixel_type("jpg"), Some("public.jpeg"));
        assert_eq!(pixel_type("JPeG"), Some("public.jpeg"));
        assert_eq!(pixel_type("bmp"), Some("public.tiff"));
        assert_eq!(pixel_type("gif"), None);
        assert_eq!(pixel_type("webp"), None);
        assert_eq!(pixel_type(""), None);
    }
}
