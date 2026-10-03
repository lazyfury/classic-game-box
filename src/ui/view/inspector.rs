//! The resource inspector section: decode a running core's memory into
//! pictures (pattern tables, palettes, nametables, sprites) and a hex dump.
//!
//! The middle column picks a view and lists the core's memory map; the right
//! column shows the decoded image or the hex page. Decoding is the app's job
//! (see `crate::app::inspect`); this module only lays out what the model
//! carries.

use igui::igui_components::{Button, Column, Component, EmptyState, Flex, Row, ScrollView, Text};
use igui::igui_core::{Color, Edges};
use igui::igui_theme::radius;
use igui::igui_theme::{space, Theme, Tone};
use igui::igui_ui::{Align, Justify, MouseFilter, SurfaceStyle};

use crate::inspect::hex::PAGE_BYTES;
use crate::ui::frame::FrameImage;
use crate::ui::model::{Action, ViewModel, INSPECTOR_HEX_VIEW, INSPECTOR_VIEWS};

use super::Page;
use super::ViewBridge;

/// The middle column: the view picker, a refresh button and the core's memory
/// map.
pub(super) fn inspector_page(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &ViewBridge,
) -> Page {
    let mut scroll = None;
    let game = model.selected_game().map(|game| game.name.clone());
    let mut column = Column::new()
        .gap(space::SM)
        .padding(Edges::all(space::MD))
        .mouse_filter(MouseFilter::Ignore)
        .child(Text::heading("资源", theme));
    let subtitle = match &game {
        Some(name) => format!("{name} · {}", model.core_name),
        None => "没有正在运行的游戏".to_string(),
    };
    column = column.child(Text::caption(subtitle, theme).tone(Tone::Muted));

    if !model.has_session {
        column = column.child(
            EmptyState::new("没有正在运行的游戏", theme)
                .description("开始一个游戏后，这里能看图案表、调色板和内存。"),
        );
        return Page {
            tree: column,
            scroll,
        };
    }
    if !model.inspector_ready {
        column = column.child(
            EmptyState::new("这个核心没有发布内存映射", theme)
                .description("核心没有实现 SET_MEMORY_MAPS，检视器没有可读的内存。"),
        );
        return Page {
            tree: column,
            scroll,
        };
    }

    // The view picker: one small button per decoder, the active one filled.
    let mut views = Row::new().gap(space::XS);
    for (index, label) in INSPECTOR_VIEWS.iter().enumerate() {
        let pick = actions.clone();
        let selected = index == model.inspector_view;
        let button = if selected {
            Button::primary(*label, theme)
        } else {
            Button::ghost(*label, theme)
        };
        views = views.child(
            button
                .mini()
                .on_click(move |_tree, _id| pick.push(Action::SelectInspectorView(index))),
        );
    }
    column = column.child(views);

    let refresh = actions.clone();
    column = column.child(
        Button::secondary("刷新", theme)
            .on_click(move |_tree, _id| refresh.push(Action::RefreshInspector)),
    );

    // The core's published memory map. A row picks the region the hex view
    // dumps, so the list doubles as the hex viewer's selector.
    let mut list = Column::new()
        .gap(space::XXS)
        .child(Text::small("内存映射", theme).tone(Tone::Muted))
        .child(
            Text::caption("点一行，用「内存」看它的字节", theme)
                .tone(Tone::Subtle)
                .max_lines(2),
        );
    for (index, row) in model.inspector_regions.iter().enumerate() {
        list = list.child(region_row(theme, index, row, model, actions));
    }
    let view = ScrollView::new(theme)
        .scrollbar(false)
        .grow(1.0)
        .child(list);
    scroll = Some(view.state());
    column = column.child(view);
    Page {
        tree: column,
        scroll,
    }
}

/// One memory region. It is a click target for the hex view, and it reads as
/// selected while the hex view is showing it.
fn region_row(
    theme: &'static dyn Theme,
    index: usize,
    row: &crate::ui::InspectorRegionRow,
    model: &ViewModel,
    actions: &ViewBridge,
) -> Row {
    let pick = actions.clone();
    let selected = model.inspector_view == INSPECTOR_HEX_VIEW && index == model.inspector_region;
    let highlight = theme.palette().surface_hover;
    Row::new()
        .align(Align::Center)
        .gap(space::SM)
        .padding(Edges::new(space::XXS, space::XS, space::XXS, space::XS))
        .dynamic_background(move |state| {
            let fill = if selected || state.hovered {
                highlight
            } else {
                Color::TRANSPARENT
            };
            SurfaceStyle::new(fill).radius(radius::SM)
        })
        .on_click(move |_tree, _id| pick.push(Action::SelectInspectorRegion(index)))
        .child(
            Text::small(row.label.as_str(), theme)
                .grow(1.0)
                .max_lines(1)
                .ellipsis(true),
        )
        .child(
            Text::caption(row.range.as_str(), theme)
                .tone(Tone::Subtle)
                .max_lines(1),
        )
        .child(
            Text::caption(row.size.as_str(), theme)
                .tone(Tone::Subtle)
                .max_lines(1),
        )
}

/// The right column: the decoded picture or the hex page, with its caption.
pub(super) fn inspector_detail(
    theme: &'static dyn Theme,
    model: &ViewModel,
    actions: &ViewBridge,
) -> Column {
    let mut column = Column::new()
        .gap(space::SM)
        .padding(Edges::all(space::MD))
        .grow(1.0)
        .mouse_filter(MouseFilter::Ignore);
    let title = INSPECTOR_VIEWS
        .get(model.inspector_view)
        .copied()
        .unwrap_or("资源");
    column = column.child(
        Row::new()
            .align(Align::Center)
            .gap(space::SM)
            .child(Text::subheading(title, theme).grow(1.0))
            .child(
                Text::caption(model.inspector_caption.as_str(), theme)
                    .tone(Tone::Muted)
                    .max_lines(1)
                    .ellipsis(true),
            ),
    );

    match &model.inspector_image {
        Some(image) => {
            column =
                column.child(FrameImage::new(image.texture, image.width, image.height).grow(1.0));
        }
        None => {
            column = column.child(
                Flex::row()
                    .align(Align::Center)
                    .justify(Justify::Center)
                    .grow(1.0)
                    .surface(SurfaceStyle::new(theme.palette().surface).radius(radius::MD))
                    .child(Text::small("没有可显示的资源。", theme).tone(Tone::Muted)),
            );
        }
    }

    if model.inspector_view == INSPECTOR_HEX_VIEW {
        column = column.child(hex_pager(theme, model, actions));
    }
    column
}

/// The hex view's paging bar. The buttons always fire; the host clamps.
fn hex_pager(theme: &'static dyn Theme, model: &ViewModel, actions: &ViewBridge) -> Row {
    let prev = actions.clone();
    let next = actions.clone();
    let offset = model.inspector_hex_offset;
    let total = model.inspector_hex_total;
    let has_prev = offset > 0;
    let has_next = offset + PAGE_BYTES < total;
    let prev_button = if has_prev {
        Button::secondary("← 上一页", theme)
    } else {
        Button::ghost("← 上一页", theme)
    };
    let next_button = if has_next {
        Button::secondary("下一页 →", theme)
    } else {
        Button::ghost("下一页 →", theme)
    };
    Row::new()
        .align(Align::Center)
        .gap(space::SM)
        .mouse_filter(MouseFilter::Ignore)
        .child(
            prev_button
                .mini()
                .on_click(move |_tree, _id| prev.push(Action::InspectorHexPage(-1))),
        )
        .child(
            Text::caption(format!("0x{offset:06X}"), theme)
                .tone(Tone::Muted)
                .grow(1.0),
        )
        .child(
            next_button
                .mini()
                .on_click(move |_tree, _id| next.push(Action::InspectorHexPage(1))),
        )
}
