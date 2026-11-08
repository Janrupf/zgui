//! The text field under a combobox's frame and a palette's band.

use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::view::AttrName;
use zgui::vocab::HasPopup;
use zgui::{component, view};

use crate::input::InputProps;
use crate::listbox::Listbox;
use crate::overlay::OverlayState;
use zgui_ui_primitives::Binding;

/// A bare [`Input`](crate::Input) that narrows the enclosing listbox.
///
/// It carries the listbox's keyboard model and the relations a reader needs: what kind of surface
/// it opens, whether that surface is showing, and which option is being walked. It draws no frame,
/// so a [`ComboboxInput`](crate::ComboboxInput) puts one around it and a
/// [`CommandInput`](crate::CommandInput) does not.
#[component]
pub(crate) fn ComboboxField(
    /// What to show while it is empty.
    #[prop(optional)]
    placeholder: Option<String>,
    /// Whether it can be typed into.
    #[prop(into, default = Signal::stored_local(false))]
    disabled: Signal<bool, LocalStorage>,
    /// What the field is called, for a reader.
    #[prop(optional)]
    label: Option<String>,
    /// The element whose text names it.
    #[prop(optional)]
    labelled_by: Option<NodeRef>,
    /// Whether it shows what the chosen option reads as while no query is open.
    ///
    /// A combobox does: its field holds the choice. A palette does not: it chooses nothing, and
    /// its field only ever holds a search.
    #[prop(default = false)]
    shows_choice: bool,
    /// Classes merged after its own.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else the caller forwarded.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    let listbox = Listbox::current();
    let state = listbox.map_or_else(
        || OverlayState::uncontrolled(false, None),
        |listbox| listbox.surface(),
    );

    // The list owns what has been typed, so the field reads it from there and writes it back
    // there rather than keeping a second copy that has to be kept in step.
    let typed = Binding::controlled(
        Signal::derive_local(move || match listbox {
            Some(listbox) if !shows_choice || (state.is_open() && listbox.query_active()) => {
                listbox.filter()
            }
            Some(listbox) => listbox.chosen_text().unwrap_or_default(),
            None => String::new(),
        }),
        move |text: String| {
            if let Some(listbox) = listbox {
                listbox.set_filter(text);
                // Typing opens it. A field that narrowed a list nobody could see would be a field
                // that does nothing.
                state.open();
            }
        },
    );
    let on_key_down = handler(
        events::KEY_DOWN,
        move |ev: &mut EventCx<'_, events::KeyDown>| {
            if disabled.get_untracked() {
                return;
            }
            if let Some(listbox) = listbox
                && listbox.handle(&ev.key)
            {
                ev.prevent_default();
                ev.stop_propagation();
            }
        },
    );

    let mut semantics = A11yBinding::new(Role::ComboBox)
        .has_popup(HasPopup::Listbox)
        .expanded(move || state.is_open())
        .controls(state.content())
        .active_descendant(move || {
            listbox.map_or(zgui::vocab::NodeId(0), |listbox| {
                zgui::vocab::NodeId(
                    listbox
                        .active_node()
                        .get()
                        .map_or(0, zgui::view::NodeId::as_u64),
                )
            })
        });
    if let Some(text) = label {
        semantics = semantics.label(text);
    }
    if let Some(target) = labelled_by {
        semantics = semantics.labelled_by(target);
    }

    let own = Attrs::new()
        .class_toggle(zgui::view::ClassName::new("zui-combobox__input"), true)
        .attribute(AttrName::new("data-state"), move || {
            Some(state.state_name().to_owned())
        })
        .a11y_from(semantics);

    view! {
        Input(
            node_ref = {state.trigger()},
            value = typed,
            placeholder = placeholder.unwrap_or_default(),
            disabled = disabled,
            on:key_down = on_key_down,
            {..own},
            {..attrs},
            class = class
        )
    }
}
