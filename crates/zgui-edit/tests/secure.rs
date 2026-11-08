//! A secure editor and a masked projection: the value stays in the model.
//!
//! The document is what every other part of the framework reads — style, layout, shaping, paint,
//! accessibility, devtools. So the property is asserted on the document: after any sequence of
//! edits, each node holds exactly the mask of its paragraph and no character of the value.

use proptest::prelude::*;
use zgui_dom::{Document, EverythingMatters, NodeIndex};
use zgui_edit::Editor;
use zgui_edit::document::{Projection, mask};
use zgui_edit::editor::Command;
use zgui_edit::select::{Granularity, Motion, Selection};
use zgui_interned::ElementName;
use zgui_vocab::ImeEvent;

/// A secure editor over `start`, projected masked into a real document.
struct Secret {
    /// The document the masked paragraphs live in.
    document: Document,
    /// Which node holds which paragraph.
    projection: Projection,
    /// The model, which holds the real text.
    editor: Editor,
}

impl Secret {
    fn new(start: &str) -> Self {
        let mut editor = Editor::new(start);
        editor.set_secure(true);
        let document = Document::new();
        let projection = document
            .edit(&EverythingMatters, |edit| {
                let root = edit.create_element(ElementName::new("root"));
                edit.insert_before(document.document_index(), root, None);
                let field = edit.create_element(ElementName::new("field"));
                edit.insert_before(root, field, None);
                let mut projection = Projection::build(edit, field, editor.buffer());
                projection.set_masked(edit, editor.buffer(), true);
                projection
            })
            .expect("the document is not poisoned");
        Self {
            document,
            projection,
            editor,
        }
    }

    /// Writes what a response changed into the document.
    fn run(&mut self, act: impl FnOnce(&mut Editor) -> zgui_edit::Response) -> zgui_edit::Response {
        let response = act(&mut self.editor);
        if let Some(splice) = &response.splice {
            let projection = &mut self.projection;
            let buffer = self.editor.buffer();
            self.document
                .edit(&EverythingMatters, |edit| {
                    projection.apply(edit, splice, buffer)
                })
                .expect("the document is not poisoned");
        }
        response
    }

    /// The text each node holds, in order.
    fn nodes(&self) -> Vec<String> {
        self.projection
            .nodes()
            .iter()
            .map(|node| text_of(&self.document, *node))
            .collect()
    }

    /// Asserts the document holds the mask of the value, and nothing of the value itself.
    fn assert_masked(&self) {
        let value = self.editor.text();
        let nodes = self.nodes();
        let paragraphs = self.editor.paragraphs();
        assert_eq!(nodes.len(), paragraphs.len(), "one node per paragraph");
        for (index, (node, paragraph)) in nodes.iter().zip(paragraphs).enumerate() {
            let mut wanted = mask::masked(paragraph);
            if index + 1 < paragraphs.len() {
                wanted.push('\n');
            }
            assert_eq!(*node, wanted, "paragraph {index} of {value:?}");
            for character in value.chars().filter(|character| *character != '\n') {
                assert!(
                    !node.contains(character),
                    "{character:?} of the value reached the document"
                );
            }
        }
    }
}

/// The text one node holds.
fn text_of(document: &Document, node: NodeIndex) -> String {
    zgui_dom::text::text_of(document.store(), node)
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn typing_writes_bullets_and_the_model_keeps_the_value() {
    let mut secret = Secret::new("");
    for letter in ["h", "u", "n"] {
        secret.run(|editor| editor.apply(Command::Insert(letter.to_owned())));
    }
    assert_eq!(secret.editor.text(), "hun");
    assert_eq!(secret.nodes(), ["\u{2022}\u{2022}\u{2022}"]);
    secret.assert_masked();
}

#[test]
fn unmasking_and_masking_again_rewrites_the_nodes_and_keeps_the_caret_and_undo() {
    let mut secret = Secret::new("");
    secret.run(|editor| editor.apply(Command::Insert("ab".to_owned())));
    secret.run(|editor| {
        editor.apply(Command::Move(Motion::new(
            Granularity::Grapheme,
            false,
            false,
        )))
    });
    let caret = secret.editor.selection();

    let projection = &mut secret.projection;
    let buffer = secret.editor.buffer();
    let changed = secret
        .document
        .edit(&EverythingMatters, |edit| {
            projection.set_masked(edit, buffer, false)
        })
        .expect("the document is not poisoned");
    secret.editor.set_secure(false);
    assert!(changed);
    assert_eq!(secret.nodes(), ["ab"]);
    assert_eq!(secret.editor.selection(), caret);
    assert_eq!(secret.projection.to_shown(secret.editor.buffer(), 1), 1);

    let projection = &mut secret.projection;
    let buffer = secret.editor.buffer();
    let again = secret
        .document
        .edit(&EverythingMatters, |edit| {
            projection.set_masked(edit, buffer, false)
        })
        .expect("the document is not poisoned");
    assert!(!again, "a projection already plain writes nothing");

    let projection = &mut secret.projection;
    let buffer = secret.editor.buffer();
    secret
        .document
        .edit(&EverythingMatters, |edit| {
            projection.set_masked(edit, buffer, true)
        })
        .expect("the document is not poisoned");
    secret.editor.set_secure(true);
    secret.assert_masked();
    assert_eq!(secret.projection.to_shown(secret.editor.buffer(), 1), 3);

    secret.run(|editor| editor.apply(Command::Undo));
    assert_eq!(secret.editor.text(), "");
    secret.assert_masked();
}

#[test]
fn copy_and_cut_give_no_text_and_remove_nothing() {
    let mut secret = Secret::new("hunter2");
    secret.editor.apply(Command::SelectAll);
    let copied = secret.run(|editor| editor.apply(Command::Copy));
    assert!(copied.handled, "the chord stays with the field");
    assert_eq!(copied.clipboard, None);
    let cut = secret.run(|editor| editor.apply(Command::Cut));
    assert!(cut.handled);
    assert_eq!(cut.clipboard, None);
    assert_eq!(
        cut.splice, None,
        "a cut that copies nothing deletes nothing"
    );
    assert_eq!(secret.editor.text(), "hunter2");
    assert_eq!(secret.editor.exportable_text(), None);

    secret.editor.set_secure(false);
    assert_eq!(secret.editor.exportable_text().as_deref(), Some("hunter2"));
}

#[test]
fn a_word_is_the_whole_value_while_secure() {
    let mut secret = Secret::new("two words");
    secret.editor.set_selection(Selection::caret(9));
    let moved = secret
        .run(|editor| editor.apply(Command::Move(Motion::new(Granularity::Word, false, false))));
    assert_eq!(
        moved.selection,
        Some(Selection::caret(0)),
        "no stop at the space"
    );

    secret.editor.set_selection(Selection::caret(9));
    secret.run(|editor| editor.apply(Command::DeleteBackwards(Granularity::Word)));
    assert_eq!(secret.editor.text(), "");
    secret.assert_masked();
}

#[test]
fn debug_output_names_no_part_of_the_value() {
    let mut secret = Secret::new("hunter2");
    secret.run(|editor| editor.apply(Command::Insert("q".to_owned())));
    secret.run(|editor| {
        editor.ime(&ImeEvent::Preedit {
            text: "zz".into(),
            cursor: None,
        })
    });
    let shown = format!("{:?}", secret.editor);
    assert!(!shown.contains("hunter"), "{shown}");
    assert!(!shown.contains("zz"), "{shown}");
    assert!(!shown.contains('q'), "{shown}");
}

#[test]
fn an_emoji_value_maps_between_the_two_offset_spaces() {
    let family = "\u{1f469}\u{200d}\u{1f4bb}";
    let value = format!("a{family}b");
    let secret = Secret::new(&value);
    secret.assert_masked();
    let buffer = secret.editor.buffer();
    assert_eq!(secret.projection.to_shown(buffer, 1 + family.len()), 6);
    assert_eq!(secret.projection.to_model(buffer, 6), 1 + family.len());
    assert_eq!(secret.projection.to_model(buffer, 9), value.len());
    assert_eq!(
        secret.projection.to_model(buffer, 4),
        1,
        "inside the second bullet snaps to its start"
    );
}

/// One thing a session does.
#[derive(Clone, Debug)]
enum Step {
    Type(String),
    Backspace,
    Delete,
    DeleteWord,
    Caret(usize),
    Select(usize, usize),
    SelectAll,
    Paste(String),
    Cut,
    Undo,
    Redo,
    Preedit(String),
    Commit(String),
    Abandon,
}

impl Step {
    fn run(&self, secret: &mut Secret) {
        let command = match self {
            Self::Type(text) => Command::Insert(text.clone()),
            Self::Backspace => Command::DeleteBackwards(Granularity::Grapheme),
            Self::Delete => Command::DeleteForwards(Granularity::Grapheme),
            Self::DeleteWord => Command::DeleteBackwards(Granularity::Word),
            Self::Caret(at) => Command::Select(Selection::caret(*at)),
            Self::Select(anchor, focus) => Command::Select(Selection::new(*anchor, *focus)),
            Self::SelectAll => Command::SelectAll,
            Self::Paste(text) => Command::Paste(text.clone()),
            Self::Cut => Command::Cut,
            Self::Undo => Command::Undo,
            Self::Redo => Command::Redo,
            Self::Preedit(text) => {
                let event = ImeEvent::Preedit {
                    text: text.as_str().into(),
                    cursor: None,
                };
                secret.run(|editor| editor.ime(&event));
                return;
            }
            Self::Commit(text) => {
                let event = ImeEvent::Commit(text.as_str().into());
                secret.run(|editor| editor.ime(&event));
                return;
            }
            Self::Abandon => {
                secret.run(|editor| editor.ime(&ImeEvent::Disabled));
                return;
            }
        };
        let response = secret.run(|editor| editor.apply(command));
        assert_eq!(response.clipboard, None, "a secure editor gave text away");
    }
}

/// Text a step types: short, with the characters that break naive offsets in it.
fn text() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            Just('a'),
            Just(' '),
            Just('\n'),
            Just('é'),
            Just('e'),
            Just('\u{301}'),
            Just('日'),
            Just('\u{1f469}'),
            Just('\u{200d}'),
            Just('\u{1f1e6}'),
        ],
        0..4,
    )
    .prop_map(|characters| characters.into_iter().collect())
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        text().prop_map(Step::Type),
        Just(Step::Backspace),
        Just(Step::Delete),
        Just(Step::DeleteWord),
        (0usize..24).prop_map(Step::Caret),
        (0usize..24, 0usize..24).prop_map(|(anchor, focus)| Step::Select(anchor, focus)),
        Just(Step::SelectAll),
        text().prop_map(Step::Paste),
        Just(Step::Cut),
        Just(Step::Undo),
        Just(Step::Redo),
        text().prop_map(Step::Preedit),
        text().prop_map(Step::Commit),
        Just(Step::Abandon),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 2_000,
        ..ProptestConfig::default()
    })]

    #[test]
    fn no_sequence_of_edits_writes_any_of_the_value_into_the_document(
        steps in proptest::collection::vec(step(), 0..12),
        start in text(),
    ) {
        let mut secret = Secret::new(&start);
        secret.assert_masked();
        for step in &steps {
            step.run(&mut secret);
            secret.assert_masked();
            let buffer = secret.editor.buffer();
            let focus = secret.editor.selection().focus;
            let shown = secret.projection.to_shown(buffer, focus);
            prop_assert!(shown <= mask::shown_len(&buffer.text()));
            prop_assert!(secret.projection.to_model(buffer, shown) <= focus);
        }
    }
}
