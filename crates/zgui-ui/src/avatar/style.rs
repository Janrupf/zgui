//! What an avatar looks like, in tokens.

use zgui::style;

style! { pub AvatarStyle =>
    // The circle is not cut out of the avatar. Cropping the picture is the picture's own business
    // — it is the only child that is square and has to be made round — and an `overflow: hidden`
    // here would take the badge with it: a mark pinned to the bottom corner of a disc lies mostly
    // *outside* that disc, so the avatar would clip away the half of it that is the whole point,
    // along with the ring that separates it from what is behind.
    ":scope {
        position: relative;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        flex: none;
        --zui-avatar-size: max(
            calc(var(--zui-space-base) * 8),
            calc(var(--zui-type-size-sm) * 16 / 7)
        );
        width: var(--zui-avatar-size);
        height: var(--zui-avatar-size);
        border-radius: var(--zui-radius-full);
        background-color: var(--zui-color-muted);
        color: var(--zui-color-muted-foreground);
        font-family: var(--zui-type-family-sans);
        font-size: var(--zui-type-size-sm);
        font-weight: var(--zui-type-weight-medium);
        line-height: var(--zui-type-leading-sm);
        user-select: none;
    }"
    // The disc follows the larger of the space unit and the type of its initials, so a theme with
    // large type and a theme with tight spacing both keep two initials inside it. The default tokens
    // give 32, 24 and 40px.
    ":scope[data-size=\"sm\"] {
        --zui-avatar-size: max(calc(var(--zui-space-base) * 6), calc(var(--zui-type-size-xs) * 2));
        font-size: var(--zui-type-size-xs);
    }"
    ":scope[data-size=\"lg\"] {
        --zui-avatar-size: max(
            calc(var(--zui-space-base) * 10),
            calc(var(--zui-type-size-sm) * 20 / 7)
        );
    }"

    ":scope > .zui-avatar__image {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 100%;
        aspect-ratio: 1;
        border-radius: var(--zui-radius-full);
        object-fit: cover;
    }"
    // A picture that failed to load is taken out of the flow rather than left as a broken box, and
    // `:broken` is the engine's own answer to that — not a load handler and a signal beside it.
    ":scope > .zui-avatar__image:broken { display: none; }"

    ":scope > .zui-avatar__fallback {
        display: flex;
        align-items: center;
        justify-content: center;
        width: 100%;
        height: 100%;
        border-radius: var(--zui-radius-full);
        background-color: var(--zui-color-muted);
        color: var(--zui-color-muted-foreground);
    }"

    // The badge is pinned to the bottom corner and lifted above the picture, and the ring it wears
    // is the page's own colour: an unringed dot on a dark photograph disappears into it.
    ":scope > .zui-avatar__badge {
        position: absolute;
        right: 0;
        bottom: 0;
        z-index: 10;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 10px;
        height: 10px;
        border-radius: var(--zui-radius-full);
        background-color: var(--zui-color-primary);
        color: var(--zui-color-primary-foreground);
        box-shadow: 0 0 0 2px var(--zui-color-background);
        user-select: none;
    }"
    ":scope > .zui-avatar__badge .zui-icon { --zui-icon-md: 8px; }"
    ":scope[data-size=\"sm\"] > .zui-avatar__badge { width: 8px; height: 8px; }"
    // At eight pixels across there is no room for a drawing, only for the dot itself.
    ":scope[data-size=\"sm\"] > .zui-avatar__badge .zui-icon { display: none; }"
    ":scope[data-size=\"lg\"] > .zui-avatar__badge { width: 12px; height: 12px; }"

    // A stack: each face laps the one before it, and the ring in the page colour is what keeps the
    // overlap legible rather than turning the row into one smear.
    ".zui-avatar-group { display: flex; flex-direction: row; align-items: center; }"
    ".zui-avatar-group > * { box-shadow: 0 0 0 2px var(--zui-color-background); }"
    ".zui-avatar-group > *:not(:first-child) { margin-left: calc(var(--zui-space-base) * -2); }"

    ".zui-avatar-group__count {
        position: relative;
        display: flex;
        flex: none;
        align-items: center;
        justify-content: center;
        width: max(calc(var(--zui-space-base) * 8), calc(var(--zui-type-size-sm) * 16 / 7));
        height: max(calc(var(--zui-space-base) * 8), calc(var(--zui-type-size-sm) * 16 / 7));
        border-radius: var(--zui-radius-full);
        background-color: var(--zui-color-muted);
        color: var(--zui-color-muted-foreground);
        font-family: var(--zui-type-family-sans);
        font-size: var(--zui-type-size-sm);
    }"
    // The count takes the size of the faces beside it rather than one of its own, so a group never
    // ends in a disc that is the wrong size for the row it closes.
    ".zui-avatar-group[data-size=\"sm\"] > .zui-avatar-group__count {
        width: max(calc(var(--zui-space-base) * 6), calc(var(--zui-type-size-xs) * 2));
        height: max(calc(var(--zui-space-base) * 6), calc(var(--zui-type-size-xs) * 2));
        font-size: var(--zui-type-size-xs);
        --zui-icon-md: 12px;
    }"
    ".zui-avatar-group[data-size=\"lg\"] > .zui-avatar-group__count {
        width: max(calc(var(--zui-space-base) * 10), calc(var(--zui-type-size-sm) * 20 / 7));
        height: max(calc(var(--zui-space-base) * 10), calc(var(--zui-type-size-sm) * 20 / 7));
        --zui-icon-md: 20px;
    }"
}
