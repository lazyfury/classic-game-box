//! Small building blocks shared by the pages: the toolbar chips, the compact
//! cover icon buttons, the minimal text field, and the grid virtualization
//! helpers.

use igui::igui_components::{Button, Component, Flex, NodeRef, Text};
use igui::igui_core::{Color, Edges};
use igui::igui_theme::{radius, space, Theme, Tone};
use igui::igui_ui::{Align, Justify, SurfaceStyle};

use crate::icons::{Icon as SvgIcon, IconName};
use crate::model::ViewModel;

use super::{media, Actions, CARD_ICON, CARD_ICON_BUTTON};

/// A chip: a small single-choice control for a toolbar, ghost at rest and
/// softly filled in the accent when selected. Always mini, so callers do not
/// repeat that.
pub(super) fn chip(theme: &'static dyn Theme, label: &str, selected: bool) -> Button {
    let button = Button::ghost(label, theme).mini();
    if selected {
        button
            .text_color(theme.palette().accent)
            .dynamic_background(move |_| {
                SurfaceStyle::new(theme.palette().selection).radius(radius::MD)
            })
    } else {
        button
    }
}

/// The wrapping group a toolbar places its chips in. Wrapping keeps a long
/// group of chips inside the middle column instead of overflowing it.
pub(super) fn chip_group() -> Flex {
    Flex::row()
        .wrap(true)
        .gap(space::XS)
        .padding(Edges::all(space::XXXS))
        .align(Align::Center)
}

/// A toolbar: a muted caption above a wrapping group of chips.
pub(super) fn chip_bar(theme: &'static dyn Theme, caption: &str, chips: Flex) -> Flex {
    Flex::column()
        .padding(Edges::ZERO)
        .gap(space::XXXS)
        .child(Text::caption(caption, theme).tone(Tone::Muted))
        .child(chips)
}

/// A minimal text field: the text with a caret bar drawn between the two
/// halves. quill has no `TextInput`, so the app owns the keyboard and this only
/// renders the current state.
pub(super) fn edit_field(theme: &'static dyn Theme, text: &str, caret: usize) -> Flex {
    let caret = caret.min(text.len());
    let (before, after) = text.split_at(caret);
    Flex::row()
        .align(Align::Center)
        .gap(0.0)
        .padding(Edges::new(space::SM, space::XS, space::SM, space::XS))
        .min_size(0.0, 26.0)
        .surface(
            SurfaceStyle::new(theme.palette().surface_raised)
                .border(theme.palette().accent)
                .radius(radius::SM),
        )
        .child(Text::small(before, theme).max_lines(1))
        .child(Text::small("|", theme).color(theme.palette().accent))
        .child(Text::small(after, theme).max_lines(1).ellipsis(true))
}

/// A small, transparent-until-hovered icon button on a coloured cover. No
/// padding, so it hugs the corner; the icon is `Ignore` for input, so the
/// click lands on the button. `tip` is registered with `actions` so the host
/// can show it on hover.
pub(super) fn icon_button(
    icon: IconName,
    color: Color,
    tip: &str,
    actions: &Actions,
    on_click: impl FnMut() + 'static,
) -> impl Component {
    let node = NodeRef::new();
    actions.tip(&node, tip);
    compact_button(on_click)
        .child(SvgIcon::new(icon, color, CARD_ICON))
        .ref_(&node)
}

/// The bare frame a compact icon button shares: a fixed square, a hover fill
/// and a click, with no padding of its own.
pub(super) fn compact_button(on_click: impl FnMut() + 'static) -> Flex {
    Flex::row()
        .align(Align::Center)
        .justify(Justify::Center)
        .gap(0.0)
        .shrink(0.0)
        .padding(Edges {
            left: 2.0,
            top: 1.0,
            right: 2.0,
            bottom: 1.0,
        })
        .min_size(CARD_ICON_BUTTON, CARD_ICON_BUTTON)
        .dynamic_background(move |state| {
            let fill = if state.hovered || state.pressed {
                media::ON_MEDIA_HOVER
            } else {
                Color::TRANSPARENT
            };
            SurfaceStyle::new(fill).radius(radius::SM)
        })
        .on_click(on_click)
}

/// An empty block of `height` logical pixels, standing in for unmounted rows.
pub(super) fn spacer(height: f32) -> Flex {
    Flex::column()
        .gap(0.0)
        .padding(Edges::ZERO)
        .min_size(0.0, height)
}

/// The inclusive row range to mount for a scroll `offset` and `viewport`,
/// given the row count and stride. One row of slack past each edge keeps a
/// partly visible row from popping in.
pub(super) fn visible_rows(offset: f32, viewport: f32, rows: usize, stride: f32) -> (usize, usize) {
    if rows == 0 || stride <= 0.0 {
        return (0, 0);
    }
    let last = rows - 1;
    let first = (offset.max(0.0) / stride).floor() as usize;
    let end = ((offset + viewport) / stride).ceil() as usize + 1;
    (first.min(last), end.min(last).max(first.min(last)))
}

/// The rows a grid of `total` items mounts: `total.div_ceil(columns)` rows,
/// windowed around the viewport by [`visible_rows`].
pub(super) fn window_for(
    total: usize,
    columns: usize,
    offset: f32,
    viewport: f32,
    stride: f32,
) -> (usize, usize) {
    let rows = total.div_ceil(columns.max(1));
    visible_rows(offset, viewport, rows, stride)
}

/// Before the first layout the viewport is unknown; assume a screenful.
pub(super) fn grid_viewport(model: &ViewModel) -> f32 {
    if model.grid_viewport > 0.0 {
        model.grid_viewport
    } else {
        640.0
    }
}

/// A short "how long ago" for a screenshot caption.
pub(super) fn format_when(created_ms: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0);
    let seconds = ((now - created_ms) / 1000).max(0);
    if seconds < 60 {
        "刚刚".to_string()
    } else if seconds < 3600 {
        format!("{} 分钟前", seconds / 60)
    } else if seconds < 86_400 {
        format!("{} 小时前", seconds / 3600)
    } else {
        format!("{} 天前", seconds / 86_400)
    }
}
