//! Putting a text node's new characters into the box that already lays them out.
//!
//! A text run's characters are copied into the box when the box is built, and nothing else in a
//! frame copies them again. Until this existed, the only way to get a new string laid out was to
//! rebuild the whole tree — so a counter incrementing, a label updating and a character typed into
//! a field each replaced every box in the document, which replaced every fragment, which made every
//! fragment compare as changed, which grew the damage to the root's ink. One keystroke repainted the
//! window.
//!
//! Rewriting the box instead keeps every name in the document stable, so the change is confined to
//! the run that changed and the lines it falls on.
//!
//! # The three things a rewrite owes, and what happens without each
//!
//! **The flattened form of the containing inline formatting context has to go.** It is held beside
//! the context's box and checked against the sequence of boxes it was flattened from — and a box
//! rewritten in place is the same box in the same position, so the check passes and the *old*
//! characters are shaped, measured and drawn. This is the failure mode the patch has that a rebuild
//! does not, because a rebuilt tree has new boxes and therefore a new sequence.
//!
//! **The layout of the box and of every ancestor has to be thrown away.** A string of a different
//! width is a different measurement, and every size computed from it upwards is stale.
//!
//! **A change the rewrite cannot express has to be refused rather than approximated.** Text
//! appearing where there was none, or disappearing entirely, changes which boxes exist — an empty
//! text node generates no box at all — and inventing or deleting one here would have to reproduce
//! anonymous wrapping, inline splitting and paint order. Those changes rebuild.

use zgui_bits::Dirty;
use zgui_dom::side::BoxKey;
use zgui_dom::{Document, NodeIndex, NodeKind};

use crate::node::kind::BoxKind;
use crate::tree::dirty::mark_dirty;
use crate::tree::store::LayoutStore;

/// What trying to rewrite a document's re-shaped text in place produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Retext {
    /// Every re-shaped node was a text node whose box was rewritten where it stood.
    ///
    /// Carries how many boxes were rewritten, which is zero when nothing owed a re-shape at all.
    Patched(u32),
    /// Something was found that changes which boxes exist, so the tree has to be built again.
    ///
    /// Carries which of the refusals it was, because they are answered differently and a caller
    /// that cannot tell them apart cannot narrow either.
    Rebuild,
}

/// What a walk of the re-shaped text found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Retexted {
    /// How many boxes had their characters rewritten where they stood.
    pub rewritten: u32,
    /// The elements whose boxes have to be made again, and nothing above them.
    ///
    /// An element owing a re-shape is one whose font moved, which changes the synthesised styles
    /// of the runs below it and the metrics every one of them is measured with — a change to what
    /// the boxes are made of **throughout its subtree**, and no further. Rebuilding the document
    /// for it reports every fragment in the document as new, which repaints the whole surface: on
    /// a component gallery that measured a quarter of a second of box building, for one element,
    /// on a frame that owed nothing else at all.
    ///
    /// A subtree here is not descended into, because making its boxes again covers everything
    /// below it.
    pub confine: Vec<NodeIndex>,
}

/// Why a rewrite was refused, and at which node.
///
/// A refusal rebuilds the whole document's boxes, which reports every fragment as new and repaints
/// the whole surface — so which refusal fired is worth more than the fact that one did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// An element owing a re-shape, which changes what the boxes below it are made of.
    Reshaped(NodeIndex),
    /// A text node whose characters appeared or disappeared, which changes whether it has a box.
    Appeared(NodeIndex),
    /// A text node whose box is not a run, so there is nothing to rewrite the characters of.
    NotARun(NodeIndex),
}

/// Rewrites the characters of every box holding text that a re-shape was marked on.
///
/// The obligations themselves are left where they are. This stage is not their only reader — the
/// fragment pass reads a re-shape to decide that a line holding different glyphs has to be painted
/// again where it stands — so retiring them here would take that decision away from the stage that
/// makes it.
///
/// # Panics
///
/// Panics if `root` names no live node of `document`.
pub fn retext(store: &mut LayoutStore, document: &Document, root: NodeIndex) -> Retext {
    // The two-way answer, for a caller with nowhere to put a subtree: an element that could have
    // been confined rebuilds the document, which is what this did before there was anywhere else
    // to put it.
    match walk(store, document, root) {
        Ok(found) if found.confine.is_empty() => Retext::Patched(found.rewritten),
        _ => Retext::Rebuild,
    }
}

/// The same walk, answering which refusal stopped it rather than only that one did.
///
/// Separate from [`retext`] so that the answer every caller acts on stays a two-way one, and the
/// detail is there for whoever is measuring why a document rebuilt.
///
/// # Panics
///
/// Panics if `root` names no live node of `document`.
pub fn walk(
    store: &mut LayoutStore,
    document: &Document,
    root: NodeIndex,
) -> Result<Retexted, Refusal> {
    let mut found = Retexted::default();
    visit(store, document, root, &mut found)?;
    Ok(found)
}

/// Services one node and everything below it, and reports whether the patch is still expressible.
///
/// The descent is over the invalidation record and retires nothing, which is what separates it from
/// the walk every phase that *owns* a bit uses: this stage shares [`Dirty::RESHAPE`] with the
/// fragment pass, and a descent that cleared it would leave that pass unable to tell a line whose
/// glyphs changed from one that merely sat still.
fn visit(
    store: &mut LayoutStore,
    document: &Document,
    index: NodeIndex,
    found: &mut Retexted,
) -> Result<(), Refusal> {
    let core = document.store().core(index);
    let (own, subtree) = core.dirty().get();
    if !(own | subtree).intersects(Dirty::RESHAPE) {
        return Ok(());
    }
    if own.contains(Dirty::RESHAPE) {
        // An element owing a re-shape is one whose *font* moved, which changes the synthesised
        // styles of the runs below it and the metrics every one of them is measured with. That is
        // a change to what the boxes are made of throughout a subtree, and it arrives with an
        // obligation to rebuild anyway; refusing here is what keeps the two answers from
        // disagreeing.
        // An element with no children generates no runs, so there is nothing below it for a
        // re-shape to be about and nothing a rebuild would produce differently. Saying so is worth
        // a line because the refusal below is not a local one: it rebuilds the *whole* box tree,
        // which reports every fragment as new, which repaints the whole surface. An element is
        // told it owes a re-shape the first time it is ever styled — it has never been shaped, so
        // there is nothing to reuse — and in a document whose elements are first styled at
        // different moments, that is one whole-surface repaint apiece. A moving box that carries
        // no text was costing 1.3 megapixels to move sixty-four by thirty-two.
        if core.kind() != NodeKind::Text {
            if core.first_child().is_some() {
                // Named rather than refused. What changed is what the boxes below this element are
                // made of, so making this element's boxes again answers it — and the walk does not
                // descend, because that rebuild covers everything under it.
                found.confine.push(index);
                return Ok(());
            }
        } else if let Err(met) = rewrite(store, document, index, found) {
            // Characters appearing where there were none, or going entirely, changes whether this
            // node has a box at all — which is its *container's* decision, since that is what does
            // the anonymous wrapping, the inline splitting and the paint order. Making the
            // container's boxes again reproduces all three, so the change is confined to it rather
            // than rebuilding the document.
            let Refusal::Appeared(_) = met else {
                return Err(met);
            };
            let Some(parent) = core.parent() else {
                return Err(met);
            };
            if document.store().core(parent).kind() != NodeKind::Element {
                return Err(met);
            }
            found.confine.push(parent);
            return Ok(());
        }
    }
    let children: Vec<NodeIndex> = core
        .dirty_children()
        .iter(document.store(), index)
        .collect();
    for child in children {
        visit(store, document, child, found)?;
    }
    Ok(())
}

/// Rewrites the boxes one text node generated, and reports whether that was possible.
fn rewrite(
    store: &mut LayoutStore,
    document: &Document,
    index: NodeIndex,
    found: &mut Retexted,
) -> Result<(), Refusal> {
    let text = zgui_dom::text::node::text_of(document.store(), index).unwrap_or_default();
    let source = document.store().key_of(index);
    let boxes = store.boxes_of(source).to_vec();
    // An empty string generates no box and a non-empty one generates a box, so either of these is
    // a box appearing or disappearing rather than a box changing.
    if text.is_empty() != boxes.is_empty() {
        return Err(Refusal::Appeared(index));
    }
    if boxes.is_empty() {
        return Ok(());
    }
    let text: Box<str> = text.into();
    for key in boxes {
        if store.node(key).kind != BoxKind::TextRun {
            return Err(Refusal::NotARun(index));
        }
        if store.node(key).text.as_deref() == Some(&*text) {
            continue;
        }
        store.get_mut(key).expect("a live box").text = Some(text.clone());
        forget_flattened(store, key);
        mark_dirty(store, key);
        found.rewritten += 1;
    }
    Ok(())
}

/// Drops the flattened form held by every box at or above `key`, and reports how many boxes it
/// reached.
///
/// Every ancestor rather than the nearest inline formatting context, because which box establishes
/// the context that holds these characters is not a question with one answer: an inline box nested
/// inside another contributes to the outer one's flattened form, and a run that became a flex item
/// is a context whose whole content is itself. The chain is as long as the tree is deep and each
/// step is one pointer write, so asking the question exactly would cost more than answering it
/// generously.
fn forget_flattened(store: &mut LayoutStore, key: BoxKey) -> u32 {
    let mut reached = 0;
    let mut next = Some(key);
    while let Some(current) = next {
        if !store.contains(current) {
            break;
        }
        store.forget_flattened(current);
        reached += 1;
        next = store.node(current).parent;
    }
    reached
}

#[cfg(test)]
mod tests {
    use zgui_arena::DocumentId;
    use zgui_css::StyleDraft;
    use zgui_dom::side::BoxKey;

    use crate::node::box_node::BoxNode;
    use crate::node::kind::{BoxKind, FormattingContext};
    use crate::tree::store::LayoutStore;

    use super::forget_flattened;

    /// A box of the given kind under `parent`.
    fn insert(store: &mut LayoutStore, parent: Option<BoxKey>, kind: BoxKind) -> BoxKey {
        let key = store.insert(BoxNode::new(
            StyleDraft::initial().build(),
            kind,
            FormattingContext::Inline,
        ));
        if let Some(parent) = parent {
            store.get_mut(key).expect("live").parent = Some(parent);
            let node = store.get_mut(parent).expect("live");
            node.children.push(key);
            node.paint_children.push(key);
        }
        key
    }

    /// The walk reaches every ancestor and not only the box that was rewritten.
    ///
    /// What it does at each one — dropping a held flattening — is asserted where it can be observed
    /// against a shaper, in `tests/patch.rs`: a memo that survived shapes the characters the box was
    /// built with, and the only oracle for that is the string that reached the shaper.
    #[test]
    fn forgetting_climbs_to_the_root_rather_than_stopping_at_the_run() {
        let mut store = LayoutStore::new(DocumentId::FIRST);
        let outer = insert(&mut store, None, BoxKind::Element);
        let wrapper = insert(&mut store, Some(outer), BoxKind::AnonymousInlineRoot);
        let run = insert(&mut store, Some(wrapper), BoxKind::TextRun);
        assert_eq!(forget_flattened(&mut store, run), 3);
        assert_eq!(forget_flattened(&mut store, wrapper), 2);
    }
}
