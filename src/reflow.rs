//! Re-wrapping comment paragraphs to the line length (`vsg_rs: reflow_comments`), an extension:
//! VSG has no such rule, so this is off unless the configuration asks for it.
//!
//! It runs on the formatted bytes, where the indentation is already decided, and only touches
//! runs of own-line `-- ` comments that read as prose: same indentation, no pragma, no marker,
//! no list or table structure. Everything else is copied through, because a comment's line
//! breaks are often the only thing holding its meaning.

use crate::FormatConfig;
use crate::doc::display_width;

/// Markers that make a comment an instruction to some tool rather than prose.
const DIRECTIVES: &[&str] = &[
    "vsg_off",
    "vsg_on",
    "vsg-rs:",
    "pragma",
    "synthesis",
    "synopsys",
    "translate_off",
    "translate_on",
    "psl",
    "coverage",
    "rtl_synthesis",
];

/// The indentation and the prose of `line`, if it is a comment line that may be re-wrapped.
fn prose(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let code = line.iter().position(|b| !b.is_ascii_whitespace())?;
    let (indent, rest) = line.split_at(code);
    // `--` and one space: `---`, `--!` and `--=` start banners and documentation comments.
    let text = rest.strip_prefix(b"--".as_slice())?;
    let text = text.strip_prefix(b" ".as_slice())?;
    let text = text.strip_suffix(b"\n".as_slice()).unwrap_or(text);
    let text = text.strip_suffix(b"\r".as_slice()).unwrap_or(text);
    let first = text.split(|b| *b == b' ').next()?;
    let lower = String::from_utf8_lossy(first).to_ascii_lowercase();
    if text.is_empty()
        || text[0].is_ascii_whitespace()
        || text.contains(&b'|')
        || text.contains(&b'\t')
        // A `-- ...` line inside a block comment must not have its `*/` moved.
        || text.windows(2).any(|w| w == b"*/" || w == b"/*")
        // Lists and enumerations: their line breaks are the structure.
        || matches!(text[0], b'*' | b'-' | b'+' | b'0'..=b'9')
        || DIRECTIVES.iter().any(|d| lower.starts_with(d))
    {
        return None;
    }
    Some((indent, text))
}

/// Greedily fill `words` into `-- ` lines of at most `width` columns.
fn fill(indent: &[u8], words: &[&[u8]], width: usize, utf8: bool, out: &mut Vec<u8>) {
    let start = |out: &mut Vec<u8>| {
        out.extend_from_slice(indent);
        out.extend_from_slice(b"--");
        display_width(indent, utf8, 0) + 2
    };
    let mut col = start(out);
    let head = col;
    for word in words {
        let w = 1 + display_width(word, utf8, 0);
        if col > head && col + w > width {
            out.push(b'\n');
            col = start(out);
        }
        out.push(b' ');
        out.extend_from_slice(word);
        col += w;
    }
    out.push(b'\n');
}

pub(crate) fn comments(source: Vec<u8>, cfg: &FormatConfig) -> Vec<u8> {
    if !cfg.reflow_comments {
        return source;
    }
    let utf8 = std::str::from_utf8(&source).is_ok();
    let crlf = source.contains(&b'\r');
    let lines: Vec<&[u8]> = source.split_inclusive(|b| *b == b'\n').collect();
    let mut out = Vec::with_capacity(source.len());
    let mut i = 0;
    let mut off = false;
    while i < lines.len() {
        // A `fmt off` region is kept byte for byte, comments included.
        let body = String::from_utf8_lossy(lines[i])
            .trim()
            .to_ascii_lowercase();
        let body = body.trim_start_matches('-').trim().to_owned();
        if body == "vsg-rs: fmt off" || body == "vsg_off" {
            off = true;
        } else if body == "vsg-rs: fmt on" || body == "vsg_on" {
            off = false;
        }
        if off {
            out.extend_from_slice(lines[i]);
            i += 1;
            continue;
        }
        // A paragraph is the run of prose comment lines that share the first line's indentation.
        let Some((indent, first)) = prose(lines[i]) else {
            out.extend_from_slice(lines[i]);
            i += 1;
            continue;
        };
        let mut words: Vec<&[u8]> = first
            .split(|b| *b == b' ')
            .filter(|w| !w.is_empty())
            .collect();
        let end = lines[i + 1..]
            .iter()
            .position(|l| prose(l).is_none_or(|(ind, _)| ind != indent))
            .map_or(lines.len(), |n| i + 1 + n);
        for line in &lines[i + 1..end] {
            let (_, text) = prose(line).expect("the run only holds prose lines");
            words.extend(text.split(|b| *b == b' ').filter(|w| !w.is_empty()));
        }
        fill(indent, &words, cfg.width, utf8, &mut out);
        i = end;
    }
    if crlf {
        // `fill` writes LF, so restore the line ending the rest of the file already uses.
        let mut with_crlf = Vec::with_capacity(out.len());
        for &b in &out {
            if b == b'\n' && with_crlf.last() != Some(&b'\r') {
                with_crlf.push(b'\r');
            }
            with_crlf.push(b);
        }
        return with_crlf;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str, width: usize) -> String {
        let cfg = FormatConfig {
            width,
            reflow_comments: true,
            ..FormatConfig::default()
        };
        String::from_utf8(comments(source.as_bytes().to_vec(), &cfg)).unwrap()
    }

    #[test]
    fn joins_and_splits_a_paragraph() {
        let source = "  -- one two\n  -- three four five\n";
        assert_eq!(run(source, 20), "  -- one two three\n  -- four five\n");
        // Wrapping is stable: re-wrapping its own output changes nothing.
        assert_eq!(run(&run(source, 20), 20), run(source, 20));
    }

    #[test]
    fn keeps_structure_and_directives() {
        for source in [
            "-- vsg_off signal_008\n-- vsg_on signal_008\n",
            "--! documented\n--! and more\n",
            "-- * one item\n-- * another item\n",
            "-- | a | table |\n",
            "  -- indented\n    -- differently\n",
        ] {
            assert_eq!(run(source, 20), source, "{source:?}");
        }
    }

    #[test]
    fn a_blank_line_separates_paragraphs() {
        assert_eq!(run("-- one\n\n-- two\n", 20), "-- one\n\n-- two\n");
    }

    #[test]
    fn keeps_a_formatter_off_region() {
        let source = "-- vsg-rs: fmt off\n-- one two three four five\n-- vsg-rs: fmt on\n";
        assert_eq!(run(source, 12), source);
    }

    #[test]
    fn leaves_code_alone() {
        let source = "entity e is\nend entity e;  -- trailing stays\n";
        assert_eq!(run(source, 10), source);
    }
}
