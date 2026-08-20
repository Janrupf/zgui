//! What a contact does to a document, and what it deliberately leaves alone.
//!
//! A finger is a pointer with one difference: it cannot rest over anything. It is touching or it is
//! not there, so it has no way to take a hover off what it put one on — which is why
//! [`PointerKind::can_hover`](zgui_vocab::PointerKind::can_hover) exists and why the router reads
//! it. Everything else a pointer does, a contact does: it presses, it activates, and its events
//! travel the same path.

mod support;

use support::{Element, Fixture, Session};
use zgui_vocab::{PointerAction, UiState};

/// Two buttons side by side, each of which says something about being hovered and pressed.
fn buttons() -> Fixture {
    Fixture::new(
        Element::new("root").children(vec![
            Element::new("first").class("control"),
            Element::new("second").class("control"),
        ]),
        "root { display: block; width: 200px }
         first, second { display: block; width: 200px; height: 40px }
         .control:hover { background-color: rgb(240, 240, 240) }
         .control:active { background-color: rgb(200, 200, 200) }",
    )
}

/// Whether one element is carrying a state bit.
fn holds(session: &Session, name: &str, state: UiState) -> bool {
    let index = session.fixture.find(name);
    session
        .fixture
        .document
        .store()
        .core(index)
        .ui_state()
        .contains(state)
}

#[test]
fn a_contact_lands_on_the_element_it_touched() {
    // The whole point of touch being a pointer kind rather than a separate event stream: a control
    // written against pointer events is reached by a finger without being written twice.
    let mut session = Session::new(buttons());
    let second = session.fixture.centre_of("second");

    let path = session.touch(second, PointerAction::Pressed, |routed| {
        routed.chain.path().to_vec()
    });

    assert!(
        path.contains(&session.fixture.key("second")),
        "the contact reached the element it landed on"
    );
}

#[test]
fn a_contact_presses_what_it_lands_on() {
    // `:active` is a press rather than a rest, and a finger presses.
    let mut session = Session::new(buttons());
    let first = session.fixture.centre_of("first");

    session.touch(first, PointerAction::Pressed, |_| ());
    assert!(holds(&session, "first", UiState::ACTIVE));

    session.touch(first, PointerAction::Released, |_| ());
    assert!(!holds(&session, "first", UiState::ACTIVE), "and lets go");
}

#[test]
fn a_contact_hovers_nothing_it_touches() {
    // A finger that wrote the bit would leave it written: it reports no movement away from what it
    // touched, so the control it landed on would stay lit for the rest of the run.
    let mut session = Session::new(buttons());
    let first = session.fixture.centre_of("first");

    for action in [
        PointerAction::Entered,
        PointerAction::Pressed,
        PointerAction::Moved,
        PointerAction::Released,
        PointerAction::Left,
    ] {
        session.touch(first, action, |_| ());
        assert!(
            !holds(&session, "first", UiState::HOVER),
            "a contact wrote the hover bit on {action:?}"
        );
    }
}

#[test]
fn a_contact_leaves_the_hover_a_mouse_put_somewhere_else() {
    // The other direction, and it is the one a machine with both devices meets: a finger that
    // cleared the bit on its way off the glass would take the hover off whatever the mouse is over.
    let mut session = Session::new(buttons());
    let second = session.fixture.centre_of("second");

    session.hover("first");
    assert!(holds(&session, "first", UiState::HOVER));

    session.touch(second, PointerAction::Pressed, |_| ());
    session.touch(second, PointerAction::Released, |_| ());
    session.touch(second, PointerAction::Left, |_| ());

    assert!(
        holds(&session, "first", UiState::HOVER),
        "the mouse is still over the first control"
    );
    assert!(!holds(&session, "second", UiState::HOVER));
}

#[test]
fn a_contact_is_a_pointer_of_its_own_and_the_mouse_keeps_its_place() {
    // Two pointers on the surface, each where it was last reported. The finger is on the surface
    // for as long as it touches, and it is no answer to "what is the pointer over".
    let mut session = Session::new(buttons());
    let first = session.fixture.centre_of("first");
    let second = session.fixture.centre_of("second");

    session.hover("first");
    session.touch(second, PointerAction::Pressed, |_| ());

    assert_eq!(session.router.pointers().all().count(), 2);
    let hovering: Vec<_> = session.router.pointers().hovering().collect();
    assert_eq!(hovering.len(), 1, "one of the two rests over anything");
    assert_eq!(hovering[0].1.x.0, first.x.0);
    assert_eq!(hovering[0].1.y.0, first.y.0);

    // And the contact is forgotten when it lifts, while the mouse stays.
    session.touch(second, PointerAction::Left, |_| ());
    assert_eq!(session.router.pointers().all().count(), 1);
}

#[test]
fn a_frame_that_moved_content_rehits_under_the_mouse_and_not_under_a_finger() {
    // `rehit` is how a control that slid out from under a cursor stops being hovered. Answering it
    // from a contact would write the hover bit in a frame with no pointer event in it at all.
    let mut session = Session::new(buttons());
    let second = session.fixture.centre_of("second");
    session.touch(second, PointerAction::Pressed, |_| ());

    let moved = {
        let filter = session.fixture.filter();
        let world = session.fixture.world(&filter);
        session.router.rehit(&world)
    };

    assert!(moved.is_empty(), "a contact moved no hover");
    assert!(!holds(&session, "second", UiState::HOVER));
}
