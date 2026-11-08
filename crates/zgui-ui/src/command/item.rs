//! One command of a palette.

use zgui::prelude::*;
use zgui::reactive::{LocalStorage, UnsyncCallback};
use zgui::{component, view};

use crate::combobox::ComboboxEntryProps;
use crate::command::SHEET;
use crate::command::style::CommandStyle;

/// One command of a [`Command`](crate::Command): an option whose `on_select` is what running it
/// is.
///
/// A [`ComboboxItem`](crate::ComboboxItem) without the tick. A palette runs commands and keeps no
/// choice, so no command is marked as the chosen one.
///
/// ```
/// # use zgui::prelude::*;
/// # use zgui::{component, view};
/// # use zgui_ui::prelude::*;
/// # #[component]
/// # fn Example() -> impl IntoView {
/// view! {
///     Command {CommandList {
///         CommandItem(value = "new", text = "New invoice") {"New invoice"}
///     }}
/// }
/// # }
/// ```
#[component]
pub fn CommandItem(
    /// What choosing it reports.
    #[prop(into)]
    value: String,
    /// What it reads as and is searched by; the value unless this says otherwise.
    #[prop(into, optional)]
    text: Option<String>,
    /// Whether it can be run.
    #[prop(into, default = Signal::stored_local(false))]
    disabled: Signal<bool, LocalStorage>,
    /// What running it does.
    #[prop(optional)]
    on_select: Option<UnsyncCallback<()>>,
    /// Classes merged after the command's own.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else the caller forwarded.
    #[prop(attrs)]
    attrs: Attrs,
    /// What it shows.
    children: ChildrenFn,
) -> impl IntoView {
    install_stylesheet(SHEET, CommandStyle::CSS);
    view! {
        ComboboxEntry(
            value = value,
            text = text,
            disabled = disabled,
            on_select = on_select,
            mark = false,
            class = "zui-command__item",
            {..attrs},
            class = class
        ) {
            {children.view()}
        }
    }
}
