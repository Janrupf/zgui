//! The property a custom element's implementation is carried in.

/// The property naming which registered implementation owns an element's box.
///
/// The value is an integer packing a registry token and two revisions, written by [`reference()`]
/// and read back by [`parts`]. It is a *name*, exactly as a canvas's property is: the trait
/// object lives in a registry beside the frame loop, because an implementation can no more cross
/// the document than a shape list can.
pub const ELEMENT: &str = "custom-element";

/// The tag a custom element is built on.
pub const TAG: &str = "custom";

/// The property naming the custom properties a custom element's implementation reads.
///
/// Text: entries separated by spaces, each a name without its `--`, or a prefix followed by `*`.
/// An element without it reads every name.
pub const READS: &str = "custom-reads";

/// Whether an element whose [`READS`] entries are `reads` reads the custom property `name`.
pub fn reads_name(reads: &str, name: &str) -> bool {
    reads
        .split_ascii_whitespace()
        .any(|entry| match entry.strip_suffix('*') {
            Some(prefix) => name.starts_with(prefix),
            None => entry == name,
        })
}

/// Packs a registry token and the two revisions into the integer [`ELEMENT`] carries.
///
/// The revisions are sixteen bits each and wrap; the cost of a wrap landing exactly on a held
/// value is one missed relayout or repaint of one element — the same declared trade the canvas
/// packing makes.
pub fn reference(token: u32, layout_revision: u16, paint_revision: u16) -> i64 {
    ((token as i64) << 32) | ((layout_revision as i64) << 16) | paint_revision as i64
}

/// Reads back what [`reference()`] packed: the token, the layout revision, the paint revision.
pub fn parts(value: i64) -> (u32, u16, u16) {
    ((value >> 32) as u32, (value >> 16) as u16, value as u16)
}

/// Which obligations a change of the packed value owes.
///
/// Presence flipping is the caller's to notice; this answers for a value that moved: a layout
/// revision that moved owes a relayout, and any movement owes a repaint.
pub fn relayouts(before: i64, after: i64) -> bool {
    parts(before).1 != parts(after).1
}

/// Whether a change of the packed value names a different implementation.
///
/// A new token owes a new box, as the property appearing does: the box captured the token when it
/// was built.
pub fn owner_changed(before: i64, after: i64) -> bool {
    parts(before).0 != parts(after).0
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_reference_survives_the_packing() {
        let packed = super::reference(7, 3, 9);
        assert_eq!(super::parts(packed), (7, 3, 9));
        assert!(super::relayouts(packed, super::reference(7, 4, 9)));
        assert!(!super::relayouts(packed, super::reference(7, 3, 10)));
        assert!(super::owner_changed(packed, super::reference(8, 3, 9)));
        assert!(!super::owner_changed(packed, super::reference(7, 4, 10)));
    }

    #[test]
    fn a_read_list_names_exact_names_and_prefixes() {
        let reads = "accent syntax-*";
        assert!(super::reads_name(reads, "accent"));
        assert!(super::reads_name(reads, "syntax-keyword"));
        assert!(!super::reads_name(reads, "accent-2"));
        assert!(!super::reads_name(reads, "hover-bg"));
    }
}
