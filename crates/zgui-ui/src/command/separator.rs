//! The rule between two groups of commands.

use zgui::prelude::*;
use zgui::{component, view};

use crate::listbox::Listbox;
use crate::menu::MenuSeparatorProps;

/// A rule between two groups of commands.
///
/// Shown only while nothing has been typed, as in cmdk: a filter can empty the groups on either
/// side of it, and a rule between two hidden groups divides nothing.
///
/// ```
/// # use zgui::prelude::*;
/// # use zgui::{component, view};
/// # use zgui_ui::prelude::*;
/// # #[component]
/// # fn Example() -> impl IntoView {
/// view! {
///     Command {CommandList {
///         CommandGroup(label = "Files") {CommandItem(value = "open") {"Open"}}
///         CommandSeparator()
///         CommandGroup(label = "Help") {CommandItem(value = "docs") {"Documentation"}}
///     }}
/// }
/// # }
/// ```
#[component]
pub fn CommandSeparator(
    /// Whether it stays while something has been typed.
    #[prop(default = false)]
    always_render: bool,
    /// Classes merged after its own.
    #[prop(into, optional)]
    class: Classes,
    /// Anything else the caller forwarded.
    #[prop(attrs)]
    attrs: Attrs,
) -> impl IntoView {
    let listbox = Listbox::current();
    let shown = move || always_render || listbox.is_none_or(|listbox| listbox.filter().is_empty());
    let attrs = StoredValue::new_local(attrs);
    let class = StoredValue::new_local(class);
    view! {
        Show(when = shown, fallback = || ()) {
            MenuSeparator({..attrs.get_value()}, class = {class.get_value()})
        }
    }
}
