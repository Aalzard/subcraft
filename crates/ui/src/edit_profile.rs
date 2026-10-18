//! `--profile-edits <file> [resolution]`: measure the real app loop while a mask-generator
//! value changes every frame, exactly like dragging its slider (same app command, compositing,
//! GPU upload, thumbnails and rendering), then write frame statistics and quit.
use std::path::PathBuf;
use std::time::{Duration, Instant};

use subcraft_app::Command;
use subcraft_doc::MaskEffectKind;

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

pub struct EditProfile {
    pub out: PathBuf,
    pub resolution: Option<u32>,
    frame: u64,
    last: Option<Instant>,
    frames: Vec<Duration>,
    syncs: Vec<Duration>,
    sync_start: Option<Instant>,
    /// Edits stopped at this time; waiting for the composite to finish catching up.
    settling: Option<Instant>,
    ui_start: Option<Instant>,
    uis: Vec<Duration>,
    mark: Option<Instant>,
    warmed: bool,
    warm_frames: u32,
    sections: std::collections::BTreeMap<&'static str, Vec<Duration>>,
}

impl EditProfile {
    pub fn new(out: PathBuf, resolution: Option<u32>) -> Self {
        ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
        Self { out, resolution, frame: 0, last: None, frames: Vec::new(), syncs: Vec::new(), sync_start: None, settling: None, ui_start: None, uis: Vec::new(), mark: None, warmed: false, warm_frames: 0, sections: Default::default() }
    }

    /// Start of the app's `ui()` call.
    pub fn ui_begin(&mut self) {
        self.ui_start = Some(Instant::now());
        self.mark = self.ui_start;
    }

    /// Time since the previous mark, attributed to `label` (a section of `ui()`).
    pub fn mark(&mut self, label: &'static str) {
        let now = Instant::now();
        if let Some(prev) = self.mark.replace(now)
            && self.frame > WARMUP
            && self.settling.is_none()
        {
            self.sections.entry(label).or_default().push(now - prev);
            for (tab, d) in TABS.with(|t| std::mem::take(&mut *t.borrow_mut())) {
                self.sections.entry(tab).or_default().push(d);
            }
        } else {
            TABS.with(|t| t.borrow_mut().clear());
        }
    }

    /// End of the app's `ui()` call (the rest of a frame is egui painting and GPU present).
    pub fn ui_end(&mut self) {
        if let Some(start) = self.ui_start.take()
            && self.frame > WARMUP
            && self.settling.is_none()
        {
            self.uis.push(start.elapsed());
        }
    }

    /// Before compositing: change the first generator found (the demo's Moss mask).
    pub fn before_sync(&mut self, state: &mut UiState) {
        // Measure steady editing: wait until the first full composite (procedural fields at
        // the profiled resolution) is done.
        if self.settling.is_some() {
            return;
        }
        if !self.warmed {
            self.warm_frames += 1;
            if self.warm_frames < 3 || state.session.composites_pending() {
                return;
            }
            self.warmed = true;
        }
        self.frame += 1;
        let now = Instant::now();
        if self.frame > WARMUP && let Some(last) = self.last {
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
        {
            self.syncs.push(start.elapsed());
        }
        if self.frames.len() < FRAMES {
            return false;
        }
        let settling = *self.settling.get_or_insert_with(Instant::now);
        if state.session.composites_pending() {
            return false;
        }
        let settle = settling.elapsed();
        let stats = |v: &mut Vec<Duration>| {
            v.sort();
            let ms = |d: Duration| d.as_secs_f64() * 1e3;
            format!("p50 {:.1} ms, p95 {:.1} ms, max {:.1} ms", ms(v[v.len() / 2]), ms(v[v.len() * 95 / 100]), ms(v[v.len() - 1]))
        };
        let report = format!(
            "frames {}: frame interval {}\nwhole ui() {}\ncomposite+upload {}\nfull refresh after the last edit {:.1} ms\n",
            self.frames.len(),
            stats(&mut self.frames),
            stats(&mut self.uis),
            stats(&mut self.syncs),
            settle.as_secs_f64() * 1e3
        );
        let mut report = report;
        for (label, v) in &mut self.sections {
            v.sort();
            let (p50, p95) = (v[v.len() / 2].as_secs_f64() * 1e3, v[v.len() * 95 / 100].as_secs_f64() * 1e3);
            report.push_str(&format!("  {label}: p50 {p50:.2} ms, p95 {p95:.2} ms\n"));
        }
        log::info!("edit profile:\n{report}");
        if let Err(e) = std::fs::write(&self.out, report) {
            log::error!("edit profile: {e}");
        }
        true
    }
}
