//! `--profile-view <file>`: measure the real app loop while the 3D view is used the way a user
//! uses it, with synthesized pointer input (no real mouse): an orbit drag, the brush cursor
//! moving over the model (plain tip and a normal-map stamp), and a paint stroke. Each phase
//! reports frame intervals, the time inside `ui()` and its sections, and the viewport render
//! encode time. Runs without vsync so frame intervals are real costs, then quits.
use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Pos2};

use crate::state::{Tool, UiState};

/// Frames per measured phase.
const FRAMES: usize = 150;
/// Frames of the warm-up before any phase is measured (after the first composite is done).
const SETTLE_FRAMES: u32 = 20;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// First composite and the Shelf thumbnails still loading.
    Warmup,
    /// Nothing happens; the UI is repainted every frame.
    Idle,
    /// RMB orbit drag across the view.
    Orbit,
    /// Brush tool, pointer moving over the model (cursor circle).
    Hover,
    /// The same with a normal-map stamp brush selected.
    HoverNormal,
    /// A paint stroke with the default brush.
    Stroke,
    Done,
}

impl Phase {
    const ORDER: [Phase; 6] = [Phase::Warmup, Phase::Idle, Phase::Orbit, Phase::Hover, Phase::HoverNormal, Phase::Stroke];
    fn label(self) -> &'static str {
        match self {
            Phase::Warmup => "warm-up",
            Phase::Idle => "idle",
            Phase::Orbit => "orbit (RMB drag)",
            Phase::Hover => "brush cursor hover",
            Phase::HoverNormal => "brush cursor hover, normal-map brush",
            Phase::Stroke => "paint stroke",
            Phase::Done => "done",
        }
    }
    fn next(self) -> Phase {
        Phase::ORDER.iter().position(|p| *p == self).and_then(|i| Phase::ORDER.get(i + 1).copied()).unwrap_or(Phase::Done)
    }
}

#[derive(Default)]
struct Samples {
    intervals: Vec<Duration>,
    uis: Vec<Duration>,
    render_ms: Vec<f32>,
    sections: std::collections::BTreeMap<&'static str, Vec<Duration>>,
    /// The slowest frames: interval, and the sections of the `ui()` call that ended it.
    slowest: Vec<(Duration, Vec<(&'static str, Duration)>)>,
}

/// Slow frames kept per phase for the report.
const SLOWEST: usize = 5;

pub struct ViewProfile {
    out: PathBuf,
    phase: Phase,
    frame: usize,
    warm_frames: u32,
    last: Option<Instant>,
    ui_start: Option<Instant>,
    mark: Option<Instant>,
    current: Samples,
    results: Vec<(&'static str, Samples)>,
    /// Pointer position of the last synthesized event (points).
    pointer: Option<Pos2>,
    started: Instant,
    /// Waiting for the composite after a phase's setup (the paint layer of the stroke phase).
    settling: bool,
    /// Sections of the current frame (for the slowest-frame list).
    frame_sections: Vec<(&'static str, Duration)>,
}

impl ViewProfile {
    pub fn new(out: PathBuf) -> Self {
        crate::edit_profile::activate();
        Self {
            out,
            phase: Phase::Warmup,
            frame: 0,
            warm_frames: 0,
            last: None,
            ui_start: None,
            mark: None,
            current: Samples::default(),
            results: Vec::new(),
            pointer: None,
            started: Instant::now(),
            settling: false,
            frame_sections: Vec::new(),
        }
    }

    fn measuring(&self) -> bool {
        !matches!(self.phase, Phase::Warmup | Phase::Done)
    }

    /// Pointer path of a phase frame inside the view: a slow loop around the centre, so the
    /// cursor stays on the model most of the time.
    fn path_point(rect: egui::Rect, frame: usize) -> Pos2 {
        let t = frame as f32 / FRAMES as f32 * std::f32::consts::TAU;
        let r = rect.size() * 0.18;
        rect.center() + egui::vec2(t.cos() * r.x, (t * 2.0).sin() * r.y)
    }

    /// Add this frame's synthesized input (called from `raw_input_hook`).
    pub fn inject(&mut self, state: &UiState, events: &mut Vec<Event>) {
        let rect = state.last_view_rect;
        if !self.measuring() || self.settling || rect.width() < 8.0 {
            return;
        }
        let pos = Self::path_point(rect, self.frame);
        let button = match self.phase {
            Phase::Orbit => Some(PointerButton::Secondary),
            Phase::Stroke => Some(PointerButton::Primary),
            _ => None,
        };
        if self.frame == 0 {
            events.push(Event::PointerMoved(pos));
            if let Some(button) = button {
                events.push(Event::PointerButton { pos, button, pressed: true, modifiers: Modifiers::NONE });
            }
        } else if self.frame < FRAMES {
            events.push(Event::PointerMoved(pos));
            if self.frame == FRAMES - 1 && let Some(button) = button {
                events.push(Event::PointerButton { pos, button, pressed: false, modifiers: Modifiers::NONE });
            }
        }
        self.pointer = Some(pos);
    }

    /// Start of the app's `ui()` call: advance the phases and record the frame interval.
    pub fn ui_begin(&mut self, state: &mut UiState, ctx: &egui::Context) {
        let now = Instant::now();
        self.ui_start = Some(now);
        self.mark = Some(now);
        ctx.request_repaint();
        match self.phase {
            Phase::Warmup => {
                self.warm_frames += 1;
                let busy = state.session.composites_pending() || state.asset_preview.is_applying() || state.shelf.busy || state.env_loader.is_some();
                if self.warm_frames >= SETTLE_FRAMES && !busy {
                    self.begin_phase(Phase::Idle, state);
                }
                return;
            }
            Phase::Done => return,
            _ => {}
        }
        let frame_sections = std::mem::take(&mut self.frame_sections);
        if self.settling {
            if !(state.session.composites_pending() || state.asset_preview.is_applying()) {
                self.settling = false;
            }
            return;
        }
        if self.frame > 0 && let Some(last) = self.last {
            let interval = now - last;
            self.current.intervals.push(interval);
            self.current.render_ms.push(state.frame_time_ms);
            let slowest = &mut self.current.slowest;
            if slowest.len() < SLOWEST || slowest.last().is_some_and(|s| s.0 < interval) {
                slowest.push((interval, frame_sections));
                slowest.sort_by(|a, b| b.0.cmp(&a.0));
                slowest.truncate(SLOWEST);
            }
        }
        self.last = Some(now);
        self.frame += 1;
        if self.frame > FRAMES + 1 {
            // One extra frame so the button release is processed before the next phase.
            let done = std::mem::take(&mut self.current);
            self.results.push((self.phase.label(), done));
            let next = self.phase.next();
            self.begin_phase(next, state);
        }
    }

    fn begin_phase(&mut self, phase: Phase, state: &mut UiState) {
        self.phase = phase;
        self.frame = 0;
        self.last = None;
        self.current = Samples::default();
        match phase {
            Phase::Hover => {
                state.session.clear_brush_stamp();
                state.tool = Tool::Brush;
            }
            Phase::Stroke => {
                // Paint where a user paints: a new paint layer at the top of the stack (the
                // selection of an opened project may be a fill or a folder). Measured once its
                // composite has settled.
                state.session.clear_brush_stamp();
                state.tool = Tool::Brush;
                let _ = state.session.execute(subcraft_app::Command::SelectLayer(None));
                let _ = state.session.execute(subcraft_app::Command::AddLayer(subcraft_app::NewLayer::Paint));
                self.settling = true;
            }
            Phase::HoverNormal => {
                state.tool = Tool::Brush;
                use subcraft_assets::starter::{ENTRIES, Kind};
                if let Some(i) = ENTRIES.iter().position(|e| e.kind == Kind::Normal && e.name == "Bolted Armor")
                    && let Some(bytes) = subcraft_assets::starter::encoded(i)
                    && let Ok(bitmap) = subcraft_image::io::decode_bitmap(bytes, Some("starter.png"))
                {
                    let _ = state.session.set_brush_stamp("Bolted Armor", subcraft_paint::StampKind::Normal, bitmap);
                    state.session.brush.settings.size = 16.0;
                }
                state.show_selected_brush(None);
            }
            Phase::Done => {
                state.session.clear_brush_stamp();
                self.write_report();
            }
            _ => {}
        }
    }

    /// Time since the previous mark, attributed to `label` (a section of `ui()`).
    pub fn mark(&mut self, label: &'static str) {
        let now = Instant::now();
        let prev = self.mark.replace(now);
        let tabs = crate::edit_profile::take_tabs();
        if let Some(prev) = prev && self.measuring() && !self.settling && self.frame > 1 {
            self.current.sections.entry(label).or_default().push(now - prev);
            self.frame_sections.push((label, now - prev));
            for (tab, d) in tabs {
                self.current.sections.entry(tab).or_default().push(d);
                self.frame_sections.push((tab, d));
            }
        }
    }

    /// End of `ui()`; true once the report is written and the app may close.
    pub fn ui_end(&mut self) -> bool {
        if let Some(start) = self.ui_start.take() && self.measuring() && !self.settling && self.frame > 1 {
            self.current.uis.push(start.elapsed());
            self.frame_sections.push(("whole ui()", start.elapsed()));
        }
        self.phase == Phase::Done
    }

    fn write_report(&mut self) {
        let ms = |d: Duration| d.as_secs_f64() * 1e3;
        let stats = |v: &mut Vec<Duration>| {
            if v.is_empty() {
                return "no frames".to_string();
            }
            v.sort();
            format!("p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms", ms(v[v.len() / 2]), ms(v[v.len() * 95 / 100]), ms(v[v.len() - 1]))
        };
        let mut report = format!("view profile ({} phases, {:.1} s total, no vsync)\n", self.results.len(), self.started.elapsed().as_secs_f64());
        for (label, s) in &mut self.results {
            let fps = if s.intervals.is_empty() { 0.0 } else { s.intervals.len() as f64 / s.intervals.iter().map(|d| d.as_secs_f64()).sum::<f64>() };
            report.push_str(&format!("== {label}: {} frames, {fps:.0} fps\n", s.intervals.len()));
            report.push_str(&format!("  frame interval: {}\n", stats(&mut s.intervals)));
            report.push_str(&format!("  whole ui(): {}\n", stats(&mut s.uis)));
            s.render_ms.sort_by(f32::total_cmp);
            if !s.render_ms.is_empty() {
                report.push_str(&format!("  viewport render encode: p50 {:.2} ms, p95 {:.2} ms\n", s.render_ms[s.render_ms.len() / 2], s.render_ms[s.render_ms.len() * 95 / 100]));
            }
            for (section, v) in &mut s.sections {
                report.push_str(&format!("  {section}: {}\n", stats(v)));
            }
            for (interval, sections) in &s.slowest {
                // Sections of the `ui()` that ended the frame; the rest of the interval was spent
                // outside `ui()` (egui painting, GPU submit and present).
                let mut top: Vec<_> = sections.iter().filter(|(_, d)| ms(*d) >= 1.0).collect();
                top.sort_by(|a, b| b.1.cmp(&a.1));
                let list: Vec<String> = top.iter().take(6).map(|(n, d)| format!("{} {:.1}", n.trim(), ms(*d))).collect();
                report.push_str(&format!("  slow frame {:.1} ms: {}\n", ms(*interval), list.join(", ")));
            }
        }
        log::info!("view profile:\n{report}");
        if let Err(e) = std::fs::write(&self.out, report) {
            log::error!("view profile: {e}");
        }
    }
}
