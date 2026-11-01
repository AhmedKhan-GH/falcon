//! Subtitles for the video viewer: SubRip (`.srt`) and WebVTT (`.vtt`)
//! files read into timed cues, with the formatting tags stripped (falcon
//! draws plain text).

use std::path::{Path, PathBuf};

/// One subtitle: shown from `start` until `end` (seconds).
#[derive(Clone, Debug, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// The subtitle files Eagle loads for a video: `<name>.vtt` and
/// `<name>.srt` beside the original in its `.info` folder, in that order.
pub fn sidecars(original: &Path) -> Vec<PathBuf> {
    ["vtt", "srt"].iter().map(|ext| original.with_extension(ext)).filter(|p| p.is_file()).collect()
}

/// Reads a subtitle file; `None` if it can't be read or holds no cues.
pub fn load(path: &Path) -> Option<Vec<Cue>> {
    let bytes = std::fs::read(path).ok()?;
    let cues = parse(&String::from_utf8_lossy(&bytes));
    (!cues.is_empty()).then_some(cues)
}

/// Parses SubRip or WebVTT text. Both are blocks separated by blank lines;
/// a block's timing line holds `-->`, the lines after it are the text.
/// Blocks without a timing line (numbers on their own, `WEBVTT`, `NOTE`,
/// `STYLE`, `REGION`) are skipped, as are cues with bad times.
pub fn parse(text: &str) -> Vec<Cue> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let mut lines = block.lines().skip_while(|l| !l.contains("-->"));
        let Some(timing) = lines.next() else { continue };
        let Some((start, end)) = timing_line(timing) else { continue };
        let body: Vec<String> = lines.map(strip_tags).collect();
        let text = body.join("\n").trim().to_string();
        if !text.is_empty() && end > start {
            cues.push(Cue { start, end, text });
        }
    }
    cues.sort_by(|a, b| a.start.total_cmp(&b.start));
    cues
}

/// `00:01:02,500 --> 00:01:04,000` (SubRip) or `01:02.500 --> 01:04.000
/// align:start` (WebVTT, with optional cue settings after the end).
fn timing_line(line: &str) -> Option<(f64, f64)> {
    let (a, b) = line.split_once("-->")?;
    let start = timestamp(a.trim())?;
    let end = timestamp(b.split_whitespace().next()?)?;
    Some((start, end))
}

/// `hh:mm:ss,mmm`, `hh:mm:ss.mmm` or `mm:ss.mmm` in seconds.
pub fn timestamp(s: &str) -> Option<f64> {
    let s = s.replace(',', ".");
    let parts: Vec<&str> = s.split(':').collect();
    let (h, m, sec) = match parts.as_slice() {
        [h, m, sec] => (h.parse::<f64>().ok()?, m.parse::<f64>().ok()?, sec.parse::<f64>().ok()?),
        [m, sec] => (0.0, m.parse::<f64>().ok()?, sec.parse::<f64>().ok()?),
        _ => return None,
    };
    if h < 0.0 || !(0.0..60.0).contains(&m) || !(0.0..60.0).contains(&sec) {
        return None;
    }
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// Drops markup: `<i>`, `<b>`, `<font …>`, `<c.class>`, WebVTT voice and
/// timestamp tags, and SubRip's `{\an8}`-style overrides. Character
/// references for `&`, `<`, `>` and the non-breaking space are decoded.
fn strip_tags(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                for d in chars.by_ref() {
                    if d == '>' {
                        break;
                    }
                }
            }
            '{' if chars.peek() == Some(&'\\') => {
                for d in chars.by_ref() {
                    if d == '}' {
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", "\u{a0}").replace("&amp;", "&")
}

/// The text of every cue showing at `t`, joined by line breaks, if any.
pub fn showing(cues: &[Cue], t: f64) -> Option<String> {
    let on: Vec<&str> = cues.iter().take_while(|c| c.start <= t).filter(|c| t < c.end).map(|c| c.text.as_str()).collect();
    (!on.is_empty()).then(|| on.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps() {
        assert_eq!(timestamp("00:01:02,500"), Some(62.5));
        assert_eq!(timestamp("01:00:00.000"), Some(3600.0));
        assert_eq!(timestamp("01:02.250"), Some(62.25));
        assert_eq!(timestamp("100:00:01.000"), Some(360_001.0), "hours may run past 99");
        assert_eq!(timestamp("00:61:00.000"), None);
        assert_eq!(timestamp("abc"), None);
        assert_eq!(timestamp("12"), None);
    }

    #[test]
    fn subrip() {
        let srt = "\u{feff}1\r\n00:00:01,000 --> 00:00:02,500\r\n<i>Hello</i> there\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\n{\\an8}Two\r\nlines &amp; more\r\n\r\n3\r\n00:00:05,000 --> 00:00:04,000\r\nbackwards\r\n";
        let cues = parse(srt);
        assert_eq!(cues.len(), 2, "a cue ending before it starts is dropped");
        assert_eq!(cues[0], Cue { start: 1.0, end: 2.5, text: "Hello there".into() });
        assert_eq!(cues[1].text, "Two\nlines & more");
    }

    #[test]
    fn webvtt() {
        let vtt = "WEBVTT - a title\n\nNOTE this is\na comment\n\nSTYLE\n::cue { color: red }\n\nintro\n00:01.000 --> 00:02.000 align:start line:0\n<v Roger>Hi <c.loud>you</c>\n\n00:00.500 --> 00:00.900\nfirst\n";
        let cues = parse(vtt);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "first", "cues come out in time order");
        assert_eq!(cues[1], Cue { start: 1.0, end: 2.0, text: "Hi you".into() });
    }

    #[test]
    fn what_shows_when() {
        let cues = parse("00:01.000 --> 00:03.000\nA\n\n00:02.000 --> 00:04.000\nB\n\n00:10.000 --> 00:11.000\nC\n");
        assert_eq!(showing(&cues, 0.5), None);
        assert_eq!(showing(&cues, 1.0).as_deref(), Some("A"));
        assert_eq!(showing(&cues, 2.5).as_deref(), Some("A\nB"), "overlapping cues stack");
        assert_eq!(showing(&cues, 3.0).as_deref(), Some("B"), "a cue ends at its end time");
        assert_eq!(showing(&cues, 10.5).as_deref(), Some("C"));
        assert_eq!(showing(&cues, 11.0), None);
        assert!(parse("").is_empty());
        assert!(parse("WEBVTT\n\n").is_empty());
    }

    #[test]
    fn sidecars_beside_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("Clip.mp4");
        std::fs::write(&video, b"").unwrap();
        assert!(sidecars(&video).is_empty());
        std::fs::write(dir.path().join("Clip.srt"), "1\n00:00:01,000 --> 00:00:02,000\nHi\n").unwrap();
        std::fs::write(dir.path().join("Clip.vtt"), "WEBVTT\n").unwrap();
        let found = sidecars(&video);
        assert_eq!(found, vec![dir.path().join("Clip.vtt"), dir.path().join("Clip.srt")], "vtt first, as Eagle adds it first");
        assert_eq!(load(&found[0]), None, "an empty file gives no cues");
        assert_eq!(load(&found[1]).map(|c| c.len()), Some(1));
    }
}
