//! CPU benchmarks for the app's `ui` frame pipeline.
//!
//! Every benchmark is named `<group>/<scenario>/<size>` so `--filter` can pick
//! a slice:
//!
//! ```bash
//! cargo bench --bench ui
//! cargo bench --bench ui -- --filter ui/scroll
//! cargo bench --bench ui -- --save-baseline benches/baseline.txt
//! cargo bench --bench ui -- --baseline benches/baseline.txt
//! ```
//!
//! `--baseline` exits with code 1 when a benchmark regresses past `--threshold`,
//! which is the CI gate. The scenarios mirror the frame loop in `cgb-app`:
//! a model change rebuilds the tree, then every repaint lays it out and paints
//! a new `DrawList`.
//!
//! These are the *library* and *settings* pages at a range of sizes. The grid
//! only mounts the visible rows, so the per-frame cost should stay flat as the
//! library grows; the benchmark is what proves it (or exposes it).

use cgb_app::ui::{Actions, CoreRow, GameRow, Section, Ui, ViewModel};
use cgb_systems::SystemId;
use igui::igui_bench::{black_box, finish, BenchRunner, RunConfig};
use igui::igui_core::{Size, ViewportSize};
use igui::igui_render::PaintContext;
use igui::igui_theme::{default_theme, Mode};

/// A fixed window size, matching `cgb-app`'s default.
const VIEWPORT: (f32, f32) = (1100.0, 760.0);

/// The library sizes to sweep. The last one is far past a real collection, to
/// show whether the virtualized grid stays flat.
const SIZES: [usize; 4] = [50, 200, 1000, 5000];

/// How far one scroll step moves the grid, in logical pixels.
const SCROLL_STEP: f32 = 4.0 * 188.0;

fn game(index: usize) -> GameRow {
    let tags = if index % 3 == 0 {
        vec!["platformer".to_string(), "classic".to_string()]
    } else {
        Vec::new()
    };
    GameRow {
        id: index as i64,
        name: format!("Game {index}"),
        file_name: format!("game{index}.nes"),
        system: SystemId::Nes,
        path: format!("/roms/game{index}.nes"),
        size: (index as u64 + 1) * 4096,
        pinned: index % 17 == 0,
        play_count: (index % 9) as i64,
        play_seconds: (index % 3600) as i64,
        last_played_at: (index as i64) * 1000,
        tags,
        screenshots: (index % 5) as i64,
        cover: None,
    }
}

/// A library page with `n` games at a plausible viewport, so the grid mounts a
/// real window instead of assuming a screenful.
fn library_model(n: usize) -> ViewModel {
    ViewModel {
        games: (0..n).map(game).collect(),
        // The app feeds this back after the first layout; the grid uses it to
        // size the mounted row window.
        grid_viewport: VIEWPORT.1,
        ..ViewModel::default()
    }
}

/// A settings page with `n` declared cores, the densest list it can hold.
fn settings_model(n: usize) -> ViewModel {
    ViewModel {
        section: Section::Settings,
        cores: (0..n)
            .map(|index| CoreRow {
                key: format!("core{index}"),
                name: format!("Core {index}"),
                system: SystemId::Nes,
                selected: index == 0,
            })
            .collect(),
        library_root: Some("/roms/dir0".to_string()),
        ..ViewModel::default()
    }
}

fn push(
    results: &mut Vec<igui::igui_bench::BenchResult>,
    result: Option<igui::igui_bench::BenchResult>,
) {
    if let Some(result) = result {
        results.push(result);
    }
}

fn main() {
    let config = RunConfig::from_env();
    let runner = BenchRunner::new(&config);
    let theme = default_theme(Mode::Dark);
    let actions = Actions::default();
    let viewport = ViewportSize::new(Size::new(VIEWPORT.0, VIEWPORT.1));
    let mut results = Vec::new();

    for &n in &SIZES {
        let model = library_model(n);

        // Build the tree from scratch (the app's first frame / a page switch).
        push(
            &mut results,
            runner.run(
                format!("ui/build/library/{n}"),
                || (),
                |_| {
                    black_box(Ui::new(theme, &model, &actions));
                },
            ),
        );

        // Rebuild in place: the app's path for any model change while mounted.
        let mut ui = Ui::new(theme, &model, &actions);
        push(
            &mut results,
            runner.run(
                format!("ui/rebuild/library/{n}"),
                || (),
                |_| {
                    ui.rebuild(theme, &model, &actions);
                    black_box(ui.tree());
                },
            ),
        );

        // Rebuild then lay out: the tree is fresh, so the layout cache is cold.
        // The gap to `ui/rebuild` is the cost a model change pays for layout.
        push(
            &mut results,
            runner.run(
                format!("ui/relayout/library/{n}"),
                || {
                    let ui = Ui::new(theme, &model, &actions);
                    (model.clone(), ui)
                },
                |(model, ui)| {
                    ui.rebuild(theme, model, &actions);
                    ui.layout(viewport);
                },
            ),
        );

        // Build plus exactly one raw layout pass, bypassing `Ui`'s scroll sync
        // (which may lay out a second time once the viewport is resolved). The
        // gap to `ui/relayout` is that second pass.
        push(
            &mut results,
            runner.run(
                format!("ui/build_layout_once/library/{n}"),
                || model.clone(),
                |model| {
                    let mut ui = Ui::new(theme, model, &actions);
                    igui::igui_ui::layout(ui.tree_mut(), viewport);
                    black_box(ui.tree());
                },
            ),
        );

        // Resolve geometry with no model change: a viewport / hover repaint.
        ui.layout(viewport);
        push(
            &mut results,
            runner.run(
                format!("ui/layout/library/{n}"),
                || (),
                |_| {
                    ui.layout(viewport);
                },
            ),
        );

        // Paint the laid-out tree into a fresh draw list.
        push(
            &mut results,
            runner.run(
                format!("ui/paint/library/{n}"),
                || (),
                |_| {
                    let mut ctx = PaintContext::new();
                    ui.paint(&mut ctx);
                    black_box(ctx.into_draw_list());
                },
            ),
        );

        // A repaint with no model change: layout (hot) then paint.
        push(
            &mut results,
            runner.run(
                format!("ui/repaint/library/{n}"),
                || (),
                |_| {
                    ui.layout(viewport);
                    let mut ctx = PaintContext::new();
                    ui.paint(&mut ctx);
                    black_box(ctx.into_draw_list());
                },
            ),
        );

        // A whole model-driven frame: rebuild, layout, paint.
        push(
            &mut results,
            runner.run(
                format!("ui/frame/library/{n}"),
                || {
                    let model = library_model(n);
                    let ui = Ui::new(theme, &model, &actions);
                    (model, ui)
                },
                |(model, ui)| {
                    ui.rebuild(theme, model, &actions);
                    ui.layout(viewport);
                    let mut ctx = PaintContext::new();
                    ui.paint(&mut ctx);
                    black_box(ctx.into_draw_list());
                },
            ),
        );

        // A scroll step that crosses a row boundary: the mounted window changes
        // and the tree rebuilds. Scroll steps that stay inside the window are
        // `ui/repaint` (the ScrollView just moves the mounted content).
        push(
            &mut results,
            runner.run(
                format!("ui/scroll/library/{n}"),
                || {
                    let mut model = library_model(n);
                    model.grid_offset = SCROLL_STEP;
                    let ui = Ui::new(theme, &model, &actions);
                    (model, ui)
                },
                |(model, ui)| {
                    model.grid_offset += SCROLL_STEP;
                    if model.grid_offset > SCROLL_STEP * 8.0 {
                        model.grid_offset = SCROLL_STEP;
                    }
                    ui.rebuild(theme, model, &actions);
                    ui.layout(viewport);
                    let mut ctx = PaintContext::new();
                    ui.paint(&mut ctx);
                    black_box(ctx.into_draw_list());
                },
            ),
        );
    }

    // The shell alone (no games): isolates the fixed chrome — header, rail,
    // status bar and console column — from the grid. If this is most of a
    // model-change frame, the chrome's cold layout is the thing to attack.
    {
        let model = library_model(0);
        push(
            &mut results,
            runner.run(
                "ui/relayout/empty",
                || model.clone(),
                |model| {
                    let mut ui = Ui::new(theme, model, &actions);
                    ui.layout(viewport);
                },
            ),
        );
    }

    for &n in &SIZES {
        let model = settings_model(n);
        push(
            &mut results,
            runner.run(
                format!("ui/build/settings/{n}"),
                || (),
                |_| {
                    black_box(Ui::new(theme, &model, &actions));
                },
            ),
        );
        let mut ui = Ui::new(theme, &model, &actions);
        ui.layout(viewport);
        push(
            &mut results,
            runner.run(
                format!("ui/frame/settings/{n}"),
                || (),
                |_| {
                    ui.rebuild(theme, &model, &actions);
                    ui.layout(viewport);
                    let mut ctx = PaintContext::new();
                    ui.paint(&mut ctx);
                    black_box(ctx.into_draw_list());
                },
            ),
        );
    }

    std::process::exit(finish(&config, &results));
}
