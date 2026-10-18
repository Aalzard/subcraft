//! `--profile-edits <file> [resolution]`: measure the real app loop while a mask-generator
//! value changes every frame, exactly like dragging its slider (same app command, compositing,
//! GPU upload, thumbnails and rendering), then write frame statistics and quit.
//!
//! Two more phases are measured: the frames while the first composite warms up (every
//! procedural field at the profiled resolution is cold) and a procedural pattern scale drag
//! (one cold field every frame).
use std::path::PathBuf;
use std::time::{Duration, Instant};

use subcraft_app::Command;
use subcraft_core::ChannelKind;
use subcraft_doc::{FillSource, LayerContent, MaskEffectKind};

use crate::state::UiState;

const WARMUP: u64 = 30;

static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
thread_local! {
    static TABS: std::cell::RefCell<Vec<(&'static str, Duration)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Record a measured duration while profiling.
pub fn record(label: &'static str, d: Duration) {
    if ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        TABS.with(|t| t.borrow_mut().push((label, d)));
    }
}

/// Time one docked panel while profiling (no cost otherwise).
pub fn time_tab<R>(label: &'static str, f: impl FnOnce() -> R) -> R {
    if !ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
        return f();
    }
    let start = Instant::now();
    let r = f();
    TABS.with(|t| t.borrow_mut().push((label, start.elapsed())));
    r
}
const FRAMES: usize = 120;
/// Frames of the pattern scale drag.
const COLD_FRAMES: usize = 40;

/// Frame intervals of a measured phase and when it started.
#[derive(Default)]
struct Phase {
    start: Option<Instant>,
    last: Option<Instant>,
    intervals: Vec<Duration>,
}

impl Phase {
    fn frame(&mut self, now: Instant) {
        self.start.get_or_insert(now);
        if let Some(last) = self.last.replace(now) {
            self.intervals.push(now - last);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    /// First composite still evaluating.
    Warmup,
    /// Generator level changes every frame.
    Steady,
    /// Edits stopped at this time; waiting for the composite to catch up.
    SettleSteady(Instant),
    /// Pattern scale changes every frame.
    Cold,
    SettleCold(Instant),
}

pub struct EditProfile {
    pub out: PathBuf,
    pub resolution: Option<u32>,
    stage: Stage,
    frame: u64,
    last: Option<Instant>,
    frames: Vec<Duration>,
    syncs: Vec<Duration>,
    sync_start: Option<Instant>,
    ui_start: Option<Instant>,
    uis: Vec<Duration>,
    mark: Option<Instant>,
    warm_frames: u32,
    warmup: Phase,
    warm_total: Duration,
    settle: Duration,
    cold: Phase,
    cold_settle: Duration,
    sections: std::collections::BTreeMap<&'static str, Vec<Duration>>,
    /// Longest time per section during the warm-up.
    warm_sections: std::collections::BTreeMap<&'static str, Duration>,
}

impl EditProfile {
    pub fn new(out: PathBuf, resolution: Option<u32>) -> Self {
        ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
        Self {
            out,
            resolution,
            stage: Stage::Warmup,
            frame: 0,
            last: None,
            frames: Vec::new(),
            syncs: Vec::new(),
            sync_start: None,
            ui_start: None,
            uis: Vec::new(),
            mark: None,
            warm_frames: 0,
            warmup: Phase::default(),
            warm_total: Duration::ZERO,
            settle: Duration::ZERO,
            cold: Phase::default(),
            cold_settle: Duration::ZERO,
            sections: Default::default(),
            warm_sections: Default::default(),
        }
    }

    /// Steady editing frames after the warm-up are the ones whose sections are reported.
    fn measuring(&self) -> bool {
        self.stage == Stage::Steady && self.frame > WARMUP
    }

    /// Start of the app's `ui()` call.
    pub fn ui_begin(&mut self) {
        self.ui_start = Some(Instant::now());
        self.mark = self.ui_start;
    }

    /// Time since the previous mark, attributed to `label` (a section of `ui()`).
    pub fn mark(&mut self, label: &'static str) {
        let now = Instant::now();
        let prev_mark = self.mark.replace(now);
        if let Some(prev) = prev_mark
            && self.measuring()
        {
            self.sections.entry(label).or_default().push(now - prev);
            for (tab, d) in TABS.with(|t| std::mem::take(&mut *t.borrow_mut())) {
                self.sections.entry(tab).or_default().push(d);
            }
        } else if let Some(prev) = prev_mark
            && self.stage == Stage::Warmup
        {
            let mut longest = |label, d: Duration| {
                let e = self.warm_sections.entry(label).or_default();
                *e = (*e).max(d);
            };
            longest(label, now - prev);
            for (tab, d) in TABS.with(|t| std::mem::take(&mut *t.borrow_mut())) {
                longest(tab, d);
            }
        } else {
            TABS.with(|t| t.borrow_mut().clear());
        }
    }

    /// End of the app's `ui()` call (the rest of a frame is egui painting and GPU present).
    pub fn ui_end(&mut self) {
        if let Some(start) = self.ui_start.take()
            && self.measuring()
        {
            self.uis.push(start.elapsed());
        }
    }

    /// Before compositing: change the first generator found (the demo's Moss mask), or in the
    /// cold phase the first procedural fill's pattern scale (the demo's Wood).
    pub fn before_sync(&mut self, state: &mut UiState) {
        let now = Instant::now();
        match self.stage {
            Stage::Warmup => {
                // Measure steady editing only once the first full composite is done.
                self.warm_frames += 1;
                self.warmup.frame(now);
                if self.warm_frames < 3 || state.session.composites_pending() {
                    return;
                }
                self.warm_total = self.warmup.start.map_or(Duration::ZERO, |s| now - s);
                self.stage = Stage::Steady;
            }
            Stage::Steady => {}
            Stage::Cold => {
                self.cold.frame(now);
                edit_pattern_scale(state, self.cold.intervals.len());
                return;
            }
            Stage::SettleSteady(_) | Stage::SettleCold(_) => return,
        }
        self.frame += 1;
        if self.frame > WARMUP
            && let Some(last) = self.last
        {
            self.frames.push(now - last);
        }
        self.last = Some(now);
        let target = state.session.document().texture_sets.iter().flat_map(|ts| ts.layers.iter()).find_map(|l| {
            let e = l.mask.as_ref()?.effects.iter().find(|e| matches!(e.kind, MaskEffectKind::Generator(_)))?;
            Some((l.id, (**e).clone()))
        });
        if let Some((layer, mut effect)) = target
            && let MaskEffectKind::Generator(g) = &mut effect.kind
        {
            g.level = 0.6 + 0.25 * (self.frame as f32 * 0.15).sin();
            let _ = state.session.execute(Command::SetMaskEffect { layer, effect });
        }
        self.sync_start = Some(Instant::now());
    }

    /// After compositing + upload. Returns true when done (stats written).
    pub fn after_sync(&mut self, state: &UiState) -> bool {
        if let Some(start) = self.sync_start.take()
            && self.frame > WARMUP
            && self.stage == Stage::Steady
        {
            self.syncs.push(start.elapsed());
        }
        let pending = state.session.composites_pending();
        match self.stage {
            Stage::Steady if self.frames.len() >= FRAMES => self.stage = Stage::SettleSteady(Instant::now()),
            Stage::SettleSteady(start) if !pending => {
                self.settle = start.elapsed();
                self.stage = Stage::Cold;
            }
            Stage::Cold if self.cold.intervals.len() >= COLD_FRAMES => self.stage = Stage::SettleCold(Instant::now()),
            Stage::SettleCold(start) if !pending => {
                self.cold_settle = start.elapsed();
                self.write_report();
                return true;
            }
            _ => {}
        }
        false
    }

    fn write_report(&mut self) {
        let ms = |d: Duration| d.as_secs_f64() * 1e3;
        // Keep chronological evidence before stats sorts the samples: startup outliers must
        // not be confused with a late cold-tile stall.
        let warm_trace: Vec<_> = self.warmup.intervals.iter().take(12).map(|d| format!("{:.1}", ms(*d))).collect();
        let warm_longest = self.warmup.intervals.iter().enumerate().max_by_key(|(_, d)| **d).map(|(i, _)| i + 1);
        let stats = |v: &mut Vec<Duration>| {
            if v.is_empty() {
                return "no frames".to_string();
            }
            v.sort();
            format!("p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms", ms(v[v.len() / 2]), ms(v[v.len() * 95 / 100]), ms(v[v.len() - 1]))
        };
        let mut report = format!(
            "warm-up (first composite): {} frames in {:.1} ms, frame interval {}\n\
             frames {}: frame interval {}\nwhole ui() {}\ncomposite+upload {}\nfull refresh after the last edit {:.1} ms\n\
             pattern scale drag, {} frames: frame interval {}\nfull refresh after the last scale edit {:.1} ms\n",
            self.warmup.intervals.len(),
            ms(self.warm_total),
            stats(&mut self.warmup.intervals),
            self.frames.len(),
            stats(&mut self.frames),
            stats(&mut self.uis),
            stats(&mut self.syncs),
            ms(self.settle),
            self.cold.intervals.len(),
            stats(&mut self.cold.intervals),
            ms(self.cold_settle),
        );
        for (label, v) in &mut self.sections {
            v.sort();
            let (p50, p95) = (v[v.len() / 2].as_secs_f64() * 1e3, v[v.len() * 95 / 100].as_secs_f64() * 1e3);
            report.push_str(&format!("  {label}: p50 {p50:.2} ms, p95 {p95:.2} ms\n"));
        }
        report.push_str(&format!("warm-up first 12 intervals (ms): {}\nwarm-up longest interval number: {:?}\n", warm_trace.join(", "), warm_longest));
        report.push_str("warm-up, longest per section:\n");
        for (label, d) in &self.warm_sections {
            report.push_str(&format!("  {label}: {:.1} ms\n", ms(*d)));
        }
        log::info!("edit profile:\n{report}");
        if let Err(e) = std::fs::write(&self.out, report) {
            log::error!("edit profile: {e}");
        }
    }
}

/// Set the first procedural base color fill's pattern scale (a new cold field every step).
fn edit_pattern_scale(state: &mut UiState, step: usize) {
    let target = state.session.document().texture_sets.iter().flat_map(|ts| ts.layers.iter()).find_map(|l| {
        let LayerContent::Fill(f) = &l.content else { return None };
        let ch = f.channels.get(&ChannelKind::BaseColor)?;
        matches!(ch.source, FillSource::Procedural { .. }).then(|| (l.id, ch.clone()))
    });
    if let Some((layer, mut fill)) = target
        && let FillSource::Procedural { params, .. } = &mut fill.source
    {
        params.scale = 3.0 + step as f32 * 0.02;
        let _ = state.session.execute(Command::SetFillChannel { layer, channel: ChannelKind::BaseColor, fill });
    }
}
