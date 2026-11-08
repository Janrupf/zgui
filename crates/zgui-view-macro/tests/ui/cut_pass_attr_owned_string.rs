//! An owned `String` in an `attr:` value, which wants an optional string: it is a present value.

extern crate zgui_view as zgui;

use zgui_view::prelude::*;
use zgui_view_macro::{component, view};

#[component]
fn Thing(#[prop(attrs)] attrs: Attrs) -> impl IntoView {
    let _ = attrs;
    ""
}

fn main() {
    let part = String::from("root");
    let _ = view! { Thing(attr:data-part = part) };
}
