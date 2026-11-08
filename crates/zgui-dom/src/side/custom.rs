//! The custom-element implementation an element names, read off the property it is carried in.
//!
//! A reader over the property map rather than storage, exactly as [`drawing`](crate::side::drawing)
//! is: the token-and-revisions reference is the one fact the document holds about a custom
//! element, and the implementation it names lives in a registry beside the frame loop that the
//! document never sees.

use zgui_vocab::{PropKey, PropValue, prop::custom};

use crate::arena::store::DocumentStore;
use crate::id::node_key::NodeKey;

/// The implementation `node` names, as its token and its layout and paint revisions.
///
/// Nothing for an ordinary element.
pub fn reference(store: &DocumentStore, node: NodeKey) -> Option<(u32, u16, u16)> {
    match store
        .columns()
        .props
        .get(node)?
        .get(PropKey::new(custom::ELEMENT))
    {
        Some(PropValue::Integer(value)) => Some(custom::parts(*value)),
        _ => None,
    }
}

/// Whether a registered custom element owns `node`'s box.
///
/// This is what classifies the box, so it is deliberately the presence half of [`reference()`]: a
/// box built from one answer and painted from the other would be a custom fragment nothing
/// answers for.
pub fn is_custom(store: &DocumentStore, node: NodeKey) -> bool {
    reference(store, node).is_some()
}

/// The custom properties the implementation `node` names says it reads, as [`custom::READS`]
/// holds them.
///
/// Nothing when it did not say, which is every name.
pub fn reads(store: &DocumentStore, node: NodeKey) -> Option<&str> {
    match store
        .columns()
        .props
        .get(node)?
        .get(PropKey::new(custom::READS))
    {
        Some(PropValue::Text(text)) => Some(text.as_str()),
        _ => None,
    }
}

/// The readers set of a [`custom::READS`] list: the bits of each name, and every name when an
/// entry is a prefix, which no set of name bits can stand for.
pub fn reads_bloom(reads: &str) -> u64 {
    reads.split_ascii_whitespace().fold(0, |bits, entry| {
        if entry.ends_with('*') {
            crate::side::readers::ALL
        } else {
            bits | crate::side::readers::name_bits(entry)
        }
    })
}

#[cfg(test)]
mod tests {
    use zgui_interned::ElementName;
    use zgui_vocab::{PropKey, PropValue, prop::custom};

    use crate::arena::document::Document;
    use crate::mutate::filter::EverythingMatters;
    use crate::node::kind::NodeKind;

    #[test]
    fn a_reference_is_read_back_and_absence_is_ordinary() {
        let mut document = Document::new();
        let index = document.append(
            document.document_index(),
            NodeKind::Element,
            ElementName::new("custom"),
        );
        let node = document.store().key_of(index);
        assert!(!super::is_custom(document.store(), node));

        document
            .edit(&EverythingMatters, |edit| {
                edit.set_property(
                    index,
                    PropKey::new(custom::ELEMENT),
                    Some(PropValue::Integer(custom::reference(5, 1, 2))),
                );
            })
            .expect("not poisoned");
        assert_eq!(super::reference(document.store(), node), Some((5, 1, 2)));
        assert!(super::is_custom(document.store(), node));
    }
}
