//! The text nodes an editor's paragraphs are written into.

use zgui_dom::{Edit, NodeIndex};

use crate::document::mask;
use crate::text::{EditText, Splice};

/// The text one paragraph's node holds: the paragraph, and the break that ends it.
///
/// Every paragraph but the last is followed by a line break, and that break is written into the
/// same node the paragraph is. Nothing else in the document could carry it — there is one node per
/// paragraph and nothing between them — and text nodes that sit in one formatting context with
/// nothing between them are laid out as one continuous run of characters. A projection that wrote
/// the paragraphs bare would answer <kbd>Enter</kbd> by moving the text after the caret into a
/// second node and leaving the screen showing exactly the line it showed before.
///
/// Nothing when there is no such paragraph.
pub fn content_of(text: &EditText, paragraph: usize) -> Option<String> {
    let content = text.paragraph(paragraph)?;
    if paragraph + 1 < text.paragraphs().len() {
        Some(format!("{content}\n"))
    } else {
        Some(content.to_owned())
    }
}

/// Which text node holds which paragraph of an editor.
///
/// Built once against an editable element and updated by every splice. A splice that replaced one
/// paragraph with one writes one node and touches nothing else, which is what keeps a keystroke
/// from re-shaping the document.
///
/// A node holds its paragraph and the break that ends it, so the text under the element, read in
/// order, is the text the editor holds — see [`content_of`].
///
/// A *masked* projection writes one bullet per grapheme in place of each paragraph (see
/// [`mask`]). The document then holds the mask, and offsets into it are *shown* offsets:
/// [`to_shown`](Projection::to_shown) and [`to_model`](Projection::to_model) convert. For a plain
/// projection both are the identity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Projection {
    /// The element the paragraphs are children of.
    root: Option<NodeIndex>,
    /// One text node per paragraph, in order.
    nodes: Vec<NodeIndex>,
    /// Whether the nodes hold the mask of each paragraph.
    masked: bool,
}

impl Projection {
    /// Writes every paragraph of `text` into fresh text nodes under `root`.
    ///
    /// The element is expected to be empty; anything already under it stays where it is and is not
    /// part of the projection, because an editor owns the nodes it made and nothing else.
    pub fn build(edit: &mut Edit<'_>, root: NodeIndex, text: &EditText) -> Self {
        let nodes = (0..text.paragraphs().len())
            .map(|paragraph| {
                let content = Self::written(false, text, paragraph).unwrap_or_default();
                let node = edit.create_text(&content);
                edit.insert_before(root, node, None);
                node
            })
            .collect();
        Self {
            root: Some(root),
            nodes,
            masked: false,
        }
    }

    /// Takes over text nodes something else already made, one per paragraph, in order.
    ///
    /// This is how an editor attaches to an element a view built: the nodes are already there and
    /// already shaped, and making a fresh set would throw both away on the first keystroke.
    ///
    /// `masked` says what the caller writes into the nodes: the caller brings each node to
    /// [`content`](Projection::content) before the first splice.
    pub fn adopt(root: NodeIndex, nodes: Vec<NodeIndex>, masked: bool) -> Self {
        Self {
            root: Some(root),
            nodes,
            masked,
        }
    }

    /// Whether the nodes hold the mask of each paragraph.
    pub fn is_masked(&self) -> bool {
        self.masked
    }

    /// The text one paragraph's node holds: [`content_of`], or its mask when masked.
    pub fn content(&self, text: &EditText, paragraph: usize) -> Option<String> {
        Self::written(self.masked, text, paragraph)
    }

    /// The text one paragraph's node holds, masked or plain.
    fn written(masked: bool, text: &EditText, paragraph: usize) -> Option<String> {
        if !masked {
            return content_of(text, paragraph);
        }
        let content = mask::masked(text.paragraph(paragraph)?);
        if paragraph + 1 < text.paragraphs().len() {
            Some(content + "\n")
        } else {
            Some(content)
        }
    }

    /// Masks or unmasks the nodes, and answers whether anything changed.
    ///
    /// Only nodes whose text differs are written. A projection already in the state asked for
    /// writes nothing.
    pub fn set_masked(&mut self, edit: &mut Edit<'_>, text: &EditText, on: bool) -> bool {
        if self.masked == on {
            return false;
        }
        for (paragraph, node) in self.nodes.iter().enumerate() {
            let before = Self::written(self.masked, text, paragraph);
            let after = Self::written(on, text, paragraph);
            if let Some(after) = after.filter(|after| before.as_ref() != Some(after)) {
                edit.set_text(*node, &after);
            }
        }
        self.masked = on;
        true
    }

    /// The offset into the document's text a model offset names.
    ///
    /// The identity when plain. When masked, an offset inside a grapheme snaps down to the start
    /// of its bullet.
    pub fn to_shown(&self, text: &EditText, offset: usize) -> usize {
        if !self.masked {
            return offset;
        }
        let position = text.position_of(offset);
        let before: usize = text.paragraphs()[..position.paragraph]
            .iter()
            .map(|paragraph| mask::shown_len(paragraph) + 1)
            .sum();
        let paragraph = text.paragraph(position.paragraph).unwrap_or_default();
        before + mask::to_shown(paragraph, position.offset)
    }

    /// The model offset an offset into the document's text names.
    ///
    /// The identity when plain. When masked, an offset inside a bullet snaps down to the start of
    /// it, and an offset past the end answers with the end of the text.
    pub fn to_model(&self, text: &EditText, shown: usize) -> usize {
        if !self.masked {
            return shown;
        }
        let mut remaining = shown;
        for (index, paragraph) in text.paragraphs().iter().enumerate() {
            let length = mask::shown_len(paragraph);
            if remaining <= length {
                return text.start_of(index) + mask::to_model(paragraph, remaining);
            }
            remaining -= length + 1;
        }
        text.len()
    }

    /// The element the paragraphs live under.
    pub fn root(&self) -> Option<NodeIndex> {
        self.root
    }

    /// The text nodes, one per paragraph.
    pub fn nodes(&self) -> &[NodeIndex] {
        &self.nodes
    }

    /// The node holding one paragraph.
    pub fn node(&self, paragraph: usize) -> Option<NodeIndex> {
        self.nodes.get(paragraph).copied()
    }

    /// Writes one splice's paragraphs into the document.
    ///
    /// Only the paragraphs the splice named are written. A splice that changed the paragraph
    /// *count* creates or removes exactly the difference, so the paragraphs on either side of the
    /// change keep the nodes — and therefore the shaped results — they already had.
    ///
    /// The break that ends a paragraph goes into that paragraph's own node, so a splice that made a
    /// paragraph the last one, or stopped it being the last one, writes it: both are inside the
    /// range a splice reports, because the paragraph whose position at the end of the text changed
    /// is one of the paragraphs the replacement touched.
    pub fn apply(&mut self, edit: &mut Edit<'_>, splice: &Splice, text: &EditText) {
        let Some(root) = self.root else {
            return;
        };
        let inserted = splice.inserted_range();
        let shared = splice.removed.len().min(splice.inserted);
        for paragraph in splice.removed.start..splice.removed.start + shared {
            if let (Some(node), Some(content)) =
                (self.node(paragraph), self.content(text, paragraph))
            {
                edit.set_text(node, &content);
            }
        }
        for paragraph in (splice.removed.start + shared)..splice.removed.end {
            let _ = paragraph;
            if let Some(node) = self.nodes.get(splice.removed.start + shared).copied() {
                edit.remove(node);
                self.nodes.remove(splice.removed.start + shared);
            }
        }
        for paragraph in (splice.removed.start + shared)..inserted.end {
            let Some(content) = self.content(text, paragraph) else {
                continue;
            };
            let node = edit.create_text(&content);
            let before = self.nodes.get(paragraph).copied();
            edit.insert_before(root, node, before);
            self.nodes.insert(paragraph.min(self.nodes.len()), node);
        }
    }
}
