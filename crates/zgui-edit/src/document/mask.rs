//! The text a secure field shows in place of its value.
//!
//! A secure field writes one [`BULLET`] per grapheme into the document and keeps the real text in
//! the model only. Line breaks stay as they are, so a masked node holds its paragraph's bullets and
//! the break that ends it.
//!
//! This gives two offset spaces. A *model* offset counts bytes of the real text; a *shown* offset
//! counts bytes of the masked text the document holds. [`to_shown`] and [`to_model`] convert
//! between them. Both snap an offset inside a grapheme or inside a bullet down to the start of it.
//!
//! The graphemes are the ones [`crate::select::grapheme`] steps over, so one arrow key moves the
//! caret over one bullet.
//!
//! ```
//! use zgui_edit::document::mask;
//!
//! let value = "e\u{301}x";
//! assert_eq!(mask::masked(value), "\u{2022}\u{2022}");
//! assert_eq!(mask::to_shown(value, 3), 3, "after the first bullet");
//! assert_eq!(mask::to_model(value, 3), 3, "after the accented e");
//! ```

use crate::select::grapheme;

/// The character shown for each grapheme of a secure value.
pub const BULLET: char = '\u{2022}';

/// How many bytes one bullet takes.
const WIDTH: usize = BULLET.len_utf8();

/// The text shown for `text`: one bullet per grapheme, line breaks kept.
pub fn masked(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * WIDTH);
    for (index, paragraph) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        for _ in 0..graphemes(paragraph) {
            out.push(BULLET);
        }
    }
    out
}

/// How many bytes the masked form of `text` takes.
pub fn shown_len(text: &str) -> usize {
    text.split('\n')
        .map(|paragraph| graphemes(paragraph) * WIDTH + 1)
        .sum::<usize>()
        - 1
}

/// The shown offset a model offset into `text` names.
///
/// An offset inside a grapheme snaps down to the start of its bullet. An offset past the end
/// answers with the end of the masked text.
pub fn to_shown(text: &str, offset: usize) -> usize {
    let mut model = 0;
    let mut shown = 0;
    for (index, paragraph) in text.split('\n').enumerate() {
        if index > 0 {
            // The break before this paragraph is one byte in both spaces.
            if offset <= model {
                return shown;
            }
            model += 1;
            shown += 1;
        }
        let mut at = 0;
        while at < paragraph.len() {
            if offset < model + grapheme::next(paragraph, at) {
                return shown;
            }
            at = grapheme::next(paragraph, at);
            shown += WIDTH;
        }
        model += paragraph.len();
        if offset <= model {
            return shown;
        }
    }
    shown
}

/// The model offset into `text` a shown offset names.
///
/// An offset inside a bullet snaps down to the start of it. An offset past the end answers with
/// the end of `text`.
pub fn to_model(text: &str, shown: usize) -> usize {
    let mut model = 0;
    let mut seen = 0;
    for (index, paragraph) in text.split('\n').enumerate() {
        if index > 0 {
            if shown <= seen {
                return model;
            }
            model += 1;
            seen += 1;
        }
        let mut at = 0;
        while at < paragraph.len() {
            if shown < seen + WIDTH {
                return model + at;
            }
            at = grapheme::next(paragraph, at);
            seen += WIDTH;
        }
        model += paragraph.len();
        if shown <= seen {
            return model;
        }
    }
    model
}

/// How many graphemes one paragraph holds.
fn graphemes(paragraph: &str) -> usize {
    let mut count = 0;
    let mut at = 0;
    while at < paragraph.len() {
        at = grapheme::next(paragraph, at);
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::{BULLET, masked, shown_len, to_model, to_shown};

    /// `count` bullets.
    fn bullets(count: usize) -> String {
        core::iter::repeat_n(BULLET, count).collect()
    }

    /// Every model offset at a grapheme boundary goes to the shown space and back unchanged.
    fn round_trips(text: &str, boundaries: &[usize]) {
        for &offset in boundaries {
            let shown = to_shown(text, offset);
            assert_eq!(to_model(text, shown), offset, "{text:?} at {offset}");
        }
        assert_eq!(to_shown(text, text.len()), shown_len(text));
        assert_eq!(masked(text).len(), shown_len(text));
    }

    #[test]
    fn ascii_is_one_bullet_per_letter() {
        assert_eq!(masked("abc"), bullets(3));
        assert_eq!(to_shown("abc", 2), 6);
        assert_eq!(to_model("abc", 6), 2);
        round_trips("abc", &[0, 1, 2, 3]);
    }

    #[test]
    fn a_combining_mark_is_part_of_its_letter() {
        let text = "e\u{301}x";
        assert_eq!(masked(text), bullets(2));
        round_trips(text, &[0, 3, 4]);
    }

    #[test]
    fn a_joined_emoji_and_a_flag_are_one_bullet_each() {
        let family = "\u{1f469}\u{200d}\u{1f4bb}";
        let flag = "\u{1f1e6}\u{1f1e8}";
        let text = format!("{family}{flag}a");
        assert_eq!(masked(&text), bullets(3));
        round_trips(
            &text,
            &[0, family.len(), family.len() + flag.len(), text.len()],
        );
    }

    #[test]
    fn nothing_masks_to_nothing() {
        assert_eq!(masked(""), "");
        assert_eq!(shown_len(""), 0);
        assert_eq!(to_shown("", 5), 0);
        assert_eq!(to_model("", 5), 0);
    }

    #[test]
    fn line_breaks_are_kept_and_count_one_byte_in_both_spaces() {
        let text = "ab\n\u{e9}";
        assert_eq!(masked(text), format!("{}\n{}", bullets(2), bullets(1)));
        assert_eq!(to_shown(text, 3), 7, "the start of the second line");
        round_trips(text, &[0, 1, 2, 3, 5]);
    }

    #[test]
    fn an_offset_inside_a_grapheme_or_a_bullet_snaps_down() {
        let text = "e\u{301}x";
        assert_eq!(to_shown(text, 1), 0, "between the letter and its mark");
        assert_eq!(to_shown(text, 2), 0, "inside the mark");
        assert_eq!(to_model(text, 1), 0);
        assert_eq!(to_model(text, 5), 3);
    }

    #[test]
    fn an_offset_past_the_end_is_the_end() {
        assert_eq!(to_shown("ab", 99), 6);
        assert_eq!(to_model("ab", 99), 2);
    }
}
