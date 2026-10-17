//! Pure rich-text edits on byte offsets of the plain text.
use std::ops::Range;

use crate::model::{Annotation, RichText, Segment};

fn len(rt: &RichText) -> usize {
    rt.0.iter().map(|s| s.text.len()).sum()
}

/// Segments covering `r` (clipped).
pub fn slice(rt: &RichText, r: Range<usize>) -> RichText {
    let mut out = Vec::new();
    let mut pos = 0;
    for s in &rt.0 {
        let (a, b) = (pos, pos + s.text.len());
        pos = b;
        let (lo, hi) = (a.max(r.start), b.min(r.end));
        if lo < hi {
            out.push(Segment {
                text: s.text[lo - a..hi - a].to_string(),
                annotations: s.annotations.clone(),
            });
        }
    }
    RichText(out)
}

pub fn concat(a: RichText, b: RichText) -> RichText {
    let mut segs = a.0;
    segs.extend(b.0);
    RichText(segs).normalized()
}

pub fn split_at(rt: &RichText, at: usize) -> (RichText, RichText) {
    (slice(rt, 0..at), slice(rt, at..len(rt)))
}

/// Annotations typed text at `at` inherits: those of the char before it, else the first char.
fn inherited(rt: &RichText, at: usize) -> Vec<Annotation> {
    let mut pos = 0;
    for s in &rt.0 {
        pos += s.text.len();
        if at <= pos && !s.text.is_empty() {
            return s.annotations.clone();
        }
    }
    rt.0.last()
        .map(|s| s.annotations.clone())
        .unwrap_or_default()
}

/// Replaces `range` with `inserted`, keeping surrounding formatting.
pub fn apply_edit(rt: &RichText, range: Range<usize>, inserted: &str) -> RichText {
    let ins = RichText(vec![Segment {
        text: inserted.to_string(),
        annotations: if range.start == 0 && range.end > 0 {
            inherited(rt, range.end)
        } else {
            inherited(rt, range.start)
        },
    }]);
    concat(
        concat(slice(rt, 0..range.start), ins),
        slice(rt, range.end..len(rt)),
    )
}

/// Byte range in `old` that changed and its replacement in `new`.
pub fn diff<'a>(old: &str, new: &'a str) -> (Range<usize>, &'a str) {
    let mut pre = old
        .char_indices()
        .zip(new.chars())
        .find(|((_, a), b)| a != b)
        .map_or(old.len().min(new.len()), |((i, _), _)| i);
    while !new.is_char_boundary(pre) {
        pre -= 1;
    }
    let max_suf = old.len().min(new.len()) - pre;
    let mut suf = old
        .bytes()
        .rev()
        .zip(new.bytes().rev())
        .take(max_suf)
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suf) || !new.is_char_boundary(new.len() - suf) {
        suf -= 1;
    }
    (pre..old.len() - suf, &new[pre..new.len() - suf])
}

fn map_range(rt: &RichText, r: Range<usize>, f: impl Fn(&mut Vec<Annotation>)) -> RichText {
    let mut mid = slice(rt, r.clone());
    for s in &mut mid.0 {
        f(&mut s.annotations);
    }
    concat(
        concat(slice(rt, 0..r.start), mid),
        slice(rt, r.end..len(rt)),
    )
}

/// Adds `ann` to `r`, or removes it when the whole range already has it.
pub fn toggle(rt: &RichText, r: Range<usize>, ann: &Annotation) -> RichText {
    if r.is_empty() {
        return rt.clone();
    }
    let all = slice(rt, r.clone()).0.iter().all(|s| s.has(ann));
    map_range(rt, r, |anns| {
        anns.retain(|a| a != ann);
        if !all {
            anns.push(ann.clone());
        }
    })
}

/// Sets (or with an empty url removes) the link on `r`.
pub fn set_link(rt: &RichText, r: Range<usize>, url: &str) -> RichText {
    map_range(rt, r, |anns| {
        anns.retain(|a| !matches!(a, Annotation::Link(_)));
        if !url.is_empty() {
            anns.push(Annotation::Link(url.to_string()));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(json: &str) -> RichText {
        RichText::from_json_str(json)
    }

    #[test]
    fn split_and_merge_keep_annotations() {
        let t = rt(r#"[["ab"],["cd",[["b"]]]]"#);
        let (l, r) = split_at(&t, 3);
        assert_eq!(l, rt(r#"[["ab"],["c",[["b"]]]]"#));
        assert_eq!(r, rt(r#"[["d",[["b"]]]]"#));
        assert_eq!(concat(l, r), t);
    }

    #[test]
    fn edit_inherits_formatting() {
        let t = rt(r#"[["ab",[["b"]]],["cd"]]"#);
        let (range, ins) = diff("abcd", "abXcd");
        assert_eq!((range.clone(), ins), (2..2, "X"));
        assert_eq!(
            apply_edit(&t, range, ins),
            rt(r#"[["abX",[["b"]]],["cd"]]"#)
        );
        let (range, ins) = diff("abcd", "ad");
        assert_eq!(apply_edit(&t, range, ins), rt(r#"[["a",[["b"]]],["d"]]"#));
        assert_eq!(diff("aaa", "aa"), (2..3, ""));
        assert_eq!(diff("é", "éé"), (2..2, "é"));
    }

    #[test]
    fn toggle_bold() {
        let t = RichText::from_plain("hello");
        let b = toggle(&t, 1..3, &Annotation::Bold);
        assert_eq!(b, rt(r#"[["h"],["el",[["b"]]],["lo"]]"#));
        assert_eq!(toggle(&b, 1..3, &Annotation::Bold), t);
        assert_eq!(
            set_link(&t, 0..5, "https://x").0[0].link(),
            Some("https://x")
        );
    }
}
