//! Window, fullscreen, frame advance and painting — methods on [`super::App`].

use super::*;

impl super::App {
    /// Write the running game's accumulated play time to the library. Safe to
    /// call often: whole seconds are drained and the fraction is kept.
    pub(super) fn flush_playtime(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let path = session.rom_path().to_path_buf();
        let seconds = session.take_played_seconds();
        if seconds <= 0 {
            return;
        }
        let path_str = path.to_string_lossy().into_owned();
        if let Some(library) = &self.library {
            let _ = library.note_playtime(&path_str, seconds);
        }
        if let Some(source) = self
            .game_source
            .iter_mut()
            .find(|game| game.path == path_str)
        {
            source.play_seconds += seconds;
        }
    }

    /// Record a game start: bump the play count, stamp the time, and refresh
    /// the rows so the "recent"/"playtime" orders move the game.
    pub(super) fn note_started(&mut self, rom_path: &Path) {
        let path = rom_path.to_string_lossy().into_owned();
        let now = now_millis();
        if let Some(library) = &self.library {
            let _ = library.note_played(&path, now);
        }
        if let Some(source) = self.game_source.iter_mut().find(|game| game.path == path) {
            source.play_count += 1;
            source.last_played_at = now;
        }
        self.rebuild_game_rows();
        self.model.selected = self
            .model
            .games
            .iter()
            .position(|game| Path::new(&game.path) == rom_path);
        // The screenshots section follows the game just started.
        self.model.screenshot_game = self.selected_game_id();
    }

    /// Toggle the immersive fullscreen play view. Entering it needs a loaded
    /// game (the view *is* the game); leaving it always works.
    pub(super) fn toggle_fullscreen(&mut self) {
        self.set_fullscreen(!self.fullscreen_target);
    }

    /// Ask the window for `on`, keeping the lightweight play view mounted until
    /// the OS animation settles (the heavy shell is rebuilt afterwards).
    pub(super) fn set_fullscreen(&mut self, on: bool) {
        let on = on && self.session.is_some();
        if self.fullscreen_target == on
            && self.pending_fullscreen.is_none()
            && self.transition.is_none()
        {
            return;
        }
        self.fullscreen_target = on;
        // Entering hides the UI now; the window request is issued on the next
        // frame, once this tree has been built and presented, so the OS
        // animates from a blank surface. Leaving keeps the play view and only
        // rebuilds the shell after the window settles.
        if on {
            self.model.ui_hidden = true;
        }
        self.dirty = true;
        self.pending_fullscreen = Some(on);
        self.hidden_painted = false;
        self.ui.request_repaint();
    }

    pub(super) fn rebuild_ui(&mut self) {
        self.ui.rebuild(self.theme, &self.model, &self.actions);
        if let Some(measurer) = self.measurer.clone() {
            self.ui.install_measurer(measurer);
        }
        // A rebuild makes a fresh tree, which does not inherit the clipboard.
        if let Some(clipboard) = self.clipboard.clone() {
            self.ui.install_clipboard(clipboard);
        }
        self.dirty = false;
    }

    pub(super) fn advance_frame(&mut self, dt: f32) {
        self.flush_drops();
        // Issue a pending window request only after the (hidden) tree was
        // presented, so the OS animates from a settled surface.
        if let Some(on) = self.pending_fullscreen {
            if self.hidden_painted {
                self.pending_fullscreen = None;
                self.hidden_painted = false;
                if let Some(window) = self.window.clone() {
                    window.set_fullscreen(on);
                }
                self.transition = Some(FullscreenTransition {
                    target: on,
                    last_activity: Instant::now(),
                });
            }
        }
        // A fullscreen transition settles once the window stops resizing; only
        // then apply the target (rebuilding the heavy shell on exit).
        if let Some(transition) = self.transition {
            if transition.last_activity.elapsed() >= Duration::from_millis(200) {
                self.transition = None;
                self.model.ui_hidden = false;
                self.model.fullscreen = transition.target;
                self.dirty = true;
            }
        }
        // Overlay timers (a message counting down) advance with the clock.
        self.ui.update(dt);
        // A running overlay needs a fresh paint each frame, not a replay.
        if self.ui.overlays_animating() {
            self.ui.request_repaint();
        }
        self.step_gamepad();
        self.sync_msaa();

        if self.rewinding {
            // Holding the rewind key steps back a couple of snapshots a frame.
            if let Some(session) = self.session.as_mut() {
                for _ in 0..2 {
                    if !session.rewind_step() {
                        break;
                    }
                }
            }
            self.ui.request_repaint();
        } else if let Some(session) = self.session.as_mut() {
            if !session.paused() {
                if let Some(backend) = self.backend.clone() {
                    session.advance(dt as f64, &mut backend.borrow_mut(), &self.input);
                }
            }
        }

        // Bank play time every so often, so a crash or a kill loses at most
        // this window rather than the whole session.
        if self.last_play_flush.elapsed() >= Duration::from_secs(15) {
            self.flush_playtime();
            self.last_play_flush = Instant::now();
        }

        if let Some(session) = self.session.as_ref() {
            self.model.frame = session.frame();
            self.model.paused = session.paused();
            self.model.has_session = true;
            // A core message (SET_MESSAGE) goes to the status line, but only
            // when it changed (a core may repeat the same message each frame).
            if let Some(message) = session.take_message() {
                if self.model.status != message {
                    self.model.set_status(message, StatusKind::Info);
                    self.dirty = true;
                }
            }
        }
        self.refresh_play_info();

        // The resource inspector re-decodes on entry / view change, and streams
        // its texture while the game runs (so OAM and nametables stay live).
        if self.model.section == Section::Inspector && !self.model.fullscreen {
            if self.inspector_dirty {
                self.inspector_dirty = false;
                self.refresh_inspector();
            } else if self
                .session
                .as_ref()
                .is_some_and(|session| !session.paused() && session.frame_index() % 8 == 0)
            {
                self.sync_inspector();
            }
        }

        if self.dirty {
            self.rebuild_ui();
            // A rebuild closes every overlay (layout anchors die with the tree),
            // so a rebuild while the assignment modal is up re-opens it with the
            // fresh device list.
            if self.assign_overlay.is_some() {
                self.open_assign_mode_overlay();
            }
        }
        // Rebuild the draw list only when something changed. A running game
        // updates its texture in place, so its frames re-submit the previous
        // list instead of laying out and painting the whole UI again.
        self.repaint = self.ui.take_repaint() || self.draw_list.is_none();
    }

    /// Recompute the play column's live info (FPS / resolution / core) and push
    /// it into the mounted text node, without rebuilding the tree.
    pub(super) fn refresh_play_info(&mut self) {
        let (frames, core_name) = match self.session.as_ref() {
            Some(session) => (session.frame_index(), session.core_name().to_string()),
            None => {
                self.fps = 0.0;
                if !self.model.info.is_empty() {
                    self.model.info.clear();
                    self.ui.set_info("");
                }
                return;
            }
        };
        // A half-second window: long enough to be stable, short enough to feel
        // live. Between ticks the mounted readout is left as is (unless it has
        // not been mounted yet).
        let now = Instant::now();
        let elapsed = now.duration_since(self.fps_time).as_secs_f32();
        let tick = elapsed >= 0.5;
        if tick {
            let instant = frames.wrapping_sub(self.fps_frames) as f32 / elapsed;
            self.fps = if self.fps <= 0.0 {
                instant
            } else {
                self.fps * 0.5 + instant * 0.5
            };
            self.fps_frames = frames;
            self.fps_time = now;
        }
        if !tick && !self.model.info.is_empty() {
            return;
        }
        let (width, height) = self
            .model
            .frame
            .as_ref()
            .map(|frame| (frame.width, frame.height))
            .unwrap_or((0, 0));
        self.model.info = format!(
            "{} FPS · {width}×{height} · {core_name}",
            self.fps.round() as i32
        );
        self.ui.set_info(&self.model.info);
    }

    /// Resolve layout when the tree changed. The library / screenshots grids
    /// mount only the rows the viewport covers, so the resolved offset and
    /// viewport go back into the model; a scroll past the mounted rows asks for
    /// one more rebuild, while scrolling inside them is just a repaint.
    pub(super) fn layout_ui(&mut self, viewport: ViewportSize) {
        if !self.repaint {
            return;
        }
        let started = Instant::now();
        self.ui.layout(viewport);
        if matches!(self.model.section, Section::Library | Section::Screenshots)
            && !self.model.fullscreen
        {
            let offset = self.ui.scroll_offset();
            let viewport_height = self.ui.scroll_viewport();
            if offset != self.model.grid_offset || viewport_height != self.model.grid_viewport {
                self.model.grid_offset = offset;
                self.model.grid_viewport = viewport_height;
                if !self.ui.grid_window_covers(&self.model) {
                    self.dirty = true;
                }
            }
        }
        self.last_layout = started.elapsed();
    }

    /// Emit this frame's commands. An unchanged UI replays the previous draw
    /// list instead of laying out and painting the tree again.
    pub(super) fn paint_ui(&mut self, paint: &mut PaintContext) {
        if self.repaint {
            let started = Instant::now();
            self.ui.paint(paint);
            self.draw_list = Some(DrawList::from(paint.draw_list().to_vec()));
            self.last_paint = started.elapsed();
            self.profile_frame(
                self.repaint,
                self.last_layout,
                self.last_paint,
                Duration::ZERO,
            );
        } else if let Some(list) = self.draw_list.as_ref() {
            paint.extend(list);
        }
        self.hidden_painted = true;
    }

    /// Record one frame into the `CGB_PERF` profiler. It prints the per-frame
    /// breakdown, audits the draw list for structural problems, and every
    /// [`PERF_REPORT_FRAMES`] prints the aggregate summary. A no-op without
    /// `CGB_PERF`.
    pub(super) fn profile_frame(
        &mut self,
        repaint: bool,
        layout_time: Duration,
        paint_time: Duration,
        submit_time: Duration,
    ) {
        let Some(profiler) = self.profiler.as_mut() else {
            return;
        };
        let commands = self
            .draw_list
            .as_ref()
            .map_or(0, |list| list.commands().len());
        let millis = |time: Duration| time.as_secs_f32() * 1000.0;
        let stats = FrameStats {
            index: profiler.next_index(),
            frame_ms: millis(layout_time) + millis(paint_time) + millis(submit_time),
            stages: StageTimes::new(
                0.0,
                millis(layout_time),
                millis(paint_time),
                millis(submit_time),
            ),
            counters: FrameCounters::new(self.ui.tree().node_count(), 0, commands, 1),
        };

        if let Some(list) = self.draw_list.as_ref() {
            let report = inspect(list, &stats);
            for finding in report.findings() {
                // `Info` findings (e.g. a zero-glyph text command) are not
                // actionable per frame; keep the log to warnings and errors.
                if finding.severity == Severity::Info {
                    continue;
                }
                eprintln!(
                    "cgb perf [{}] {}",
                    finding.severity.label(),
                    finding.summary()
                );
            }
        }

        eprintln!(
            "cgb perf: repaint={repaint} layout={:.3}ms paint={:.3}ms submit={:.3}ms \
             nodes={} commands={commands}",
            stats.stages.layout_ms,
            stats.stages.paint_ms,
            stats.stages.render_ms,
            stats.counters.scene_nodes,
        );

        profiler.record(stats);
        if profiler.total_recorded() % PERF_REPORT_FRAMES == 0 {
            if let Some(summary) = profiler.summary() {
                eprintln!(
                    "cgb perf summary: {} frames avg={:.2}ms max={:.2}ms fps={:.1} \
                     layout={:.2}ms paint={:.2}ms submit={:.2}ms commands<={}",
                    summary.frames,
                    summary.avg_frame_ms,
                    summary.max_frame_ms,
                    summary.fps(),
                    summary.avg_stages.layout_ms,
                    summary.avg_stages.paint_ms,
                    summary.avg_stages.render_ms,
                    summary.max_draw_commands,
                );
            }
        }
    }
}
