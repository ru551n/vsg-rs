//! Safety net for formatting: the output must parse without errors and contain the same tokens
//! (keywords compared case-insensitively) and the same comments, in the same order, as the
//! input. Any difference is a formatter defect and the output is discarded.

use vhdl_syntax::syntax::TokenKind;

use crate::Parsed;
use crate::format::comments;

struct Entry {
    kind: TokenKind,
    text: Vec<u8>,
    /// (is trailing, text) of the comments before the token.
    comments: Vec<(bool, Vec<u8>)>,
}

fn signature(parsed: &Parsed) -> Vec<Entry> {
    parsed
        .tokens()
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut text = t.text().as_bytes().to_vec();
            if matches!(t.kind(), TokenKind::Keyword(_)) {
                text.make_ascii_lowercase();
            }
            let comments = comments(t, i > 0)
                .0
                .into_iter()
                .map(|c| (c.trailing, c.text.to_vec()))
                .collect();
            Entry {
                kind: t.kind(),
                text,
                comments,
            }
        })
        .collect()
}

impl Entry {
    fn same(&self, other: &Entry) -> bool {
        self.kind == other.kind && self.text == other.text && self.comments == other.comments
    }

    fn describe(&self) -> String {
        let comments: Vec<String> = self
            .comments
            .iter()
            .map(|(trailing, c)| {
                let kind = if *trailing { "trailing" } else { "leading" };
                format!("{kind} {}", String::from_utf8_lossy(c))
            })
            .collect();
        format!(
            "{:?} {:?} {comments:?}",
            self.kind,
            String::from_utf8_lossy(&self.text)
        )
    }
}

pub(crate) fn equivalent_parsed(before: &Parsed, after: &Parsed) -> Result<(), String> {
    if let Some(e) = after.errors.first().filter(|_| before.errors.is_empty()) {
        return Err(format!("formatted output does not parse: {}", e.message));
    }
    let (a, b) = (signature(before), signature(after));
    if a.len() != b.len() {
        return Err(format!(
            "token count changed from {} to {}",
            a.len(),
            b.len()
        ));
    }
    match a.iter().zip(&b).position(|(x, y)| !x.same(y)) {
        None => Ok(()),
        Some(i) => Err(format!(
            "token {i} changed: {} became {}",
            a[i].describe(),
            b[i].describe()
        )),
    }
}
