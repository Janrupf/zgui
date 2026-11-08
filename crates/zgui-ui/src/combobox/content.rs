//! The list a combobox narrows.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, StoredValue};
use zgui::view::ClassName;
use zgui::{component, view};
use zgui_ui_primitives::{Align, Placement, Side};

use crate::combobox::SHEET;
use crate::combobox::style::ComboboxStyle;
use crate::listbox::{Listbox, ListboxCatalogueOfProps};
use crate::overlay::{AnchoredSurfaceProps, OverlayState};

/// The options of a [`Combobox`](crate::Combobox), under its field.
///
/// ```
/// # use zgui::prelude::*;
/// # use zgui::{component, view};
/// # use zgui_ui::prelude::*;
/// # #[component]
/// # fn Example() -> impl IntoView {
/// view! {
///     Combobox {
///         ComboboxInput()
///         ComboboxContent {ComboboxItem(value = "gb") {"United Kingdom"}}
///     }
/// }
/// # }
/// ```
#[component]
pub fn ComboboxContent(
    /// Where it is asked to go, before the window's edges have their say.
    #[prop(into, default = Signal::stored_local(Placement::new(Side::Bottom, Align::Start)))]
    placement: Signal<Placement, LocalStorage>,
    /// How far off the field it sits, in pixels.
    #[prop(default = 4.0)]
    offset: f32,
    /// Classes merged after the list's own.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else the caller forwarded, which lands on the list.
    #[prop(attrs)]
    attrs: Attrs,
    /// The options.
    children: ChildrenFn,
) -> impl IntoView {
    install_stylesheet(SHEET, ComboboxStyle::CSS);
    let listbox = Listbox::current();
    let state = listbox.map_or_else(
        || OverlayState::uncontrolled(false, None),
        |listbox| listbox.surface(),
    );
    let own = Attrs::new().class_toggle(ClassName::new("zui-combobox__list"), true);
    // Held rather than moved: the options are built in two places — the open list, and the hidden
    // catalogue that teaches the closed field what its value reads as.
    let children = StoredValue::new_local(children);

    let list = view! {
        AnchoredSurface(
            state = state,
            placement = placement,
            offset = offset,
            role = {Role::ListBox},
            // The caret belongs to the field, and moving it into the list would stop the user
            // typing — so nothing here is confined and nothing here is focused.
            dismiss_on_outside_press = {true},
            {..own},
            {..attrs},
            class = class
        ) {
            {children.get_value().view()}
            ComboboxQueryEnd()
        }
    };
    // Built exactly while the list is not, so a value chosen before the list ever opened still
    // reads as its option's text. See [`SelectContent`](crate::SelectContent) for why.
    let described = view! {
        if move || listbox.is_some() && !state.is_open() {
            ListboxCatalogueOf {{children.get_value().view()}}
        } else {}
    };

    (list.into_view(), described.into_view())
}

/// Ends the query when the list it narrows has gone, so the next opening shows every option.
///
/// Built with the list's content, so its cleanup runs after the exit animation, when nobody can
/// see the options come back.
#[component]
fn ComboboxQueryEnd() -> impl IntoView {
    if let Some(listbox) = Listbox::current() {
        on_cleanup_local(move || listbox.reset_query());
    }
}

/// What a combobox says when nothing survives the filter.
///
/// It shows itself only when the list is empty, so it is never a row among options — and it is a
/// live region, so a reader is told the search found nothing without having to go looking.
///
/// ```
/// # use zgui::prelude::*;
/// # use zgui::{component, view};
/// # use zgui_ui::prelude::*;
/// # #[component]
/// # fn Example() -> impl IntoView {
/// view! {
///     Combobox {ComboboxContent {
///         ComboboxEmpty {"Nothing by that name."}
///     }}
/// }
/// # }
/// ```
#[component]
pub fn ComboboxEmpty(
    /// Classes merged after its own.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else the caller forwarded.
    #[prop(attrs)]
    attrs: Attrs,
    /// What it says.
    children: ChildrenFn,
) -> impl IntoView {
    install_stylesheet(SHEET, ComboboxStyle::CSS);
    let listbox = Listbox::current();
    let nothing = move || listbox.is_some_and(|listbox| listbox.entries().is_empty());
    let own = StoredValue::new_local(
        Attrs::new()
            .class_toggle(ClassName::new("zui-combobox__empty"), true)
            .a11y_from(A11yBinding::unspecified().live(zgui::vocab::Live::Polite))
            .merged(attrs),
    );
    let class = StoredValue::new_local(class);

    view! {
        Show(when = nothing, fallback = || ()) {
            box({..own.get_value()}, class = {class.get_value()}) {{children.view()}}
        }
    }
}
