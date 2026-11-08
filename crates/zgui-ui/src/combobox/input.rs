//! The field a combobox is narrowed with.

use zgui::prelude::*;
use zgui::reactive::LocalStorage;
use zgui::{component, view};
use zgui_ui_icons::prelude::*;
use zgui_ui_icons::set::chevron::CHEVRON_DOWN;

use crate::combobox::SHEET;
use crate::combobox::field::ComboboxFieldProps;
use crate::combobox::style::ComboboxStyle;
use crate::input_group::{
    InputGroupAddonAlign, InputGroupAddonProps, InputGroupProps, PARTS_SHEET,
    style::InputGroupPartStyle,
};

/// The text field of a [`Combobox`](crate::Combobox).
///
/// It is an [`Input`](crate::Input) with the listbox's keyboard model over it and the relations a
/// reader needs: what kind of surface it opens, whether that surface is showing, and which option
/// is being walked.
///
/// While the list is open and something has been typed, it shows what was typed. Otherwise it
/// shows what the chosen option reads as.
///
/// ```
/// # use zgui::prelude::*;
/// # use zgui::{component, view};
/// # use zgui_ui::prelude::*;
/// # #[component]
/// # fn Example() -> impl IntoView {
/// view! {
///     Combobox {
///         ComboboxInput(placeholder = "Search", label = "Country")
///         ComboboxContent {ComboboxItem(value = "gb") {"United Kingdom"}}
///     }
/// }
/// # }
/// ```
#[component]
pub fn ComboboxInput(
    /// What to show while it is empty.
    #[prop(into, optional)]
    placeholder: Option<String>,
    /// Whether it can be typed into.
    #[prop(into, default = Signal::stored_local(false))]
    disabled: Signal<bool, LocalStorage>,
    /// What the field is called, for a reader.
    #[prop(into, optional)]
    label: Option<String>,
    /// The element whose text names it.
    #[prop(optional)]
    labelled_by: Option<NodeRef>,
    /// Classes merged after its own.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else the caller forwarded.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    install_stylesheet(SHEET, ComboboxStyle::CSS);
    install_stylesheet(PARTS_SHEET, InputGroupPartStyle::CSS);

    // The frame is the group's and the field inside it is bare, so the field, the mark beside it
    // and the ring around the pair read as one control — which is what a combobox is, and what a
    // field with a chevron floating next to it is not.
    view! {
        InputGroup(disabled = disabled) {
            ComboboxField(
                placeholder = placeholder,
                disabled = disabled,
                label = label,
                labelled_by = labelled_by,
                shows_choice = true,
                class = "zui-input-group__field",
                {..attrs},
                class = class
            )
            InputGroupAddon(align = InputGroupAddonAlign::InlineEnd) {
                Icon(icon = CHEVRON_DOWN, class = "zui-combobox__mark")
            }
        }
    }
}
