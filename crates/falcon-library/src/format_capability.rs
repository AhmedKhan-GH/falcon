//! Centralized format-capability registry for Falcon.
//!
//! Categorizes file formats by preview capabilities (raster decoders, HDR,
//! RAW photos, layered PSD/PSB, Affinity documents, vectors/design files,
//! documents, 3D models, audio, video, fonts, web archives, text/code, and
//! unsupported / companion sidecar files).

/// High-level capability class for an asset's file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatCapability {
    /// Standard raster formats decoded directly by Falcon's built-in decoders
    /// or SVG renderer.
    Raster,
    /// High Dynamic Range tone-mapped imagery.
    Hdr,
    /// Adobe Photoshop layered formats.
    Layered,
    /// Camera RAW digital negatives.
    Raw,
    /// Serif Affinity suite documents.
    Affinity,
    /// Vector and specialized design packages with embedded previews or PDF streams.
    VectorDesign,
    /// Document formats.
    Document,
    /// 3D meshes and geometry models.
    Model3D,
    /// Audio recordings and tracks.
    Audio,
    /// Video and motion clips.
    Video,
    /// Typeface font files.
    Font,
    /// Web URLs and browser archives.
    Web,
    /// Plain text, markdown, configuration, and source code.
    Text,
    /// Unsupported binaries or companion / sidecar files with no preview capability.
    Unsupported,
}

/// Identifies the format capability for a given file extension.
pub fn capability(ext: &str) -> FormatCapability {
    let clean = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    match clean.as_str() {
        // Built-in raster decoders & SVG
        "png" | "jpg" | "jpeg" | "jfif" | "jpe" | "webp" | "gif" | "bmp" | "svg" | "ico" | "icns" | "tiff"
        | "tif" | "tga" | "insp" => FormatCapability::Raster,

        // High Dynamic Range
        "hdr" | "exr" => FormatCapability::Hdr,

        // Photoshop layered
        "psd" | "psdt" | "psb" => FormatCapability::Layered,

        // Camera RAW formats
        "3fr" | "arw" | "orf" | "mrw" | "nef" | "nrw" | "raw" | "cr2" | "crw" | "dng" | "erf" | "pef"
        | "raf" | "rw2" | "sr2" | "srw" | "x3f" | "cr3" | "kdc" | "mos" | "rwl" | "iiq" | "mef" => {
            FormatCapability::Raw
        }

        // Serif Affinity suite
        "af" | "afdesign" | "afphoto" | "afpub" | "aftemplate" => FormatCapability::Affinity,

        // Vector, CAD, and layout design
        "ai" | "ait" | "eps" | "sketch" | "xmind" | "key" | "idml" | "indd" | "indt" | "dwg" | "skp" => {
            FormatCapability::VectorDesign
        }

        // Documents
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "pages" | "numbers" | "rtf" => {
            FormatCapability::Document
        }

        // 3D models
        "obj" | "stl" | "ply" | "glb" | "gltf" | "3mf" | "fbx" | "dae" | "3ds" | "ifc" | "blend" => {
            FormatCapability::Model3D
        }

        // Audio
        "mp3" | "wav" | "flac" | "ogg" | "oga" | "aac" | "m4a" | "aiff" | "aif" | "opus" | "wma" => {
            FormatCapability::Audio
        }

        // Video
        "mp4" | "mov" | "m4v" | "webm" | "mkv" | "avi" | "wmv" | "mpg" | "mpeg" | "mts" | "m2ts" | "flv"
        | "f4v" | "ts" | "3gp" | "360" | "afx" | "vap" | "eva" | "vob" | "ogv" => FormatCapability::Video,

        // Fonts
        "ttf" | "otf" | "ttc" | "woff" | "woff2" => FormatCapability::Font,

        // Web
        "url" | "html" | "htm" | "mhtml" | "webloc" => FormatCapability::Web,

        // Text & Code
        "txt" | "md" | "markdown" | "json" | "xml" | "yaml" | "yml" | "toml" | "csv" | "tsv" | "css"
        | "js" | "rs" | "py" | "c" | "cpp" | "h" | "hpp" | "sh" | "zsh" | "log" | "ini" | "conf" => {
            FormatCapability::Text
        }

        // Everything else is unsupported (including sidecars and unknown binaries)
        _ => FormatCapability::Unsupported,
    }
}

/// Whether Falcon has preview or playback capabilities for this extension.
pub fn is_preview_supported(ext: &str) -> bool {
    capability(ext) != FormatCapability::Unsupported
}

/// Whether the format is known to be a companion metadata, sidecar, or lock file.
pub fn is_sidecar(ext: &str) -> bool {
    let clean = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    matches!(
        clean.as_str(),
        "xmp" | "dop" | "acr" | "aae" | "pp3" | "on1" | "cos" | "nksc" | "af~lock~" | "tmp" | "lock" | "bak"
    )
}

/// Whether the format is a visual media format suitable for thumbnail and folder cover display.
pub fn is_visual_preview(ext: &str) -> bool {
    matches!(
        capability(ext),
        FormatCapability::Raster
            | FormatCapability::Hdr
            | FormatCapability::Layered
            | FormatCapability::Raw
            | FormatCapability::Affinity
            | FormatCapability::VectorDesign
            | FormatCapability::Document
            | FormatCapability::Model3D
            | FormatCapability::Video
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_preview_formats() {
        // Raster
        assert!(is_preview_supported("png"));
        assert!(is_preview_supported("JPG"));
        assert!(is_preview_supported(".webp"));
        assert!(is_preview_supported("svg"));

        // Affinity suite
        assert!(is_preview_supported("afdesign"));
        assert!(is_preview_supported("afphoto"));
        assert!(is_preview_supported("afpub"));
        assert!(is_preview_supported("aftemplate"));
        assert_eq!(capability("afphoto"), FormatCapability::Affinity);

        // RAW
        assert!(is_preview_supported("cr3"));
        assert!(is_preview_supported("nef"));
        assert!(is_preview_supported("dng"));
        assert!(is_preview_supported("arw"));

        // PSD & Vectors
        assert!(is_preview_supported("psd"));
        assert!(is_preview_supported("ai"));
        assert!(is_preview_supported("eps"));

        // 3D & Documents
        assert!(is_preview_supported("obj"));
        assert!(is_preview_supported("glb"));
        assert!(is_preview_supported("pdf"));

        // Audio & Video
        assert!(is_preview_supported("mp3"));
        assert!(is_preview_supported("mp4"));

        // Text & Code
        assert!(is_preview_supported("txt"));
        assert!(is_preview_supported("md"));
        assert!(is_preview_supported("json"));
    }

    #[test]
    fn sidecar_and_unsupported_formats() {
        // Known sidecars
        assert!(!is_preview_supported("xmp"));
        assert!(!is_preview_supported("dop"));
        assert!(!is_preview_supported("acr"));
        assert!(!is_preview_supported("aae"));
        assert!(!is_preview_supported("pp3"));
        assert!(!is_preview_supported("on1"));
        assert!(!is_preview_supported("cos"));
        assert!(!is_preview_supported("af~lock~"));
        assert!(!is_preview_supported("tmp"));

        assert!(is_sidecar("xmp"));
        assert!(is_sidecar(".dop"));
        assert!(is_sidecar("pp3"));
        assert!(is_sidecar("af~lock~"));
        assert!(!is_sidecar("png"));
        assert!(!is_sidecar("afphoto"));

        // Arbitrary unsupported binaries
        assert!(!is_preview_supported("bin"));
        assert!(!is_preview_supported("exe"));
        assert!(!is_preview_supported("dll"));
        assert!(!is_preview_supported("dat"));
        assert_eq!(capability("exe"), FormatCapability::Unsupported);
    }

    #[test]
    fn visual_preview_classification() {
        assert!(is_visual_preview("png"));
        assert!(is_visual_preview("afphoto"));
        assert!(is_visual_preview("cr3"));
        assert!(is_visual_preview("psd"));
        assert!(is_visual_preview("pdf"));
        assert!(is_visual_preview("mp4"));
        assert!(is_visual_preview("obj"));

        // Non-visual formats are supported for preview/playback in their viewers,
        // but not prioritized as visual folder covers
        assert!(!is_visual_preview("mp3"));
        assert!(!is_visual_preview("txt"));
        assert!(!is_visual_preview("xmp"));
    }
}
