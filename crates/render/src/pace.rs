//! Keeps every GPU compute submission short (issue #119).
//!
//! Windows resets the graphics driver (TDR: the screen freezes and goes dark, every device on
//! the adapter is lost) when one submission keeps the GPU busy for about two seconds; a compute
//! dispatch cannot be interrupted. Work that is fast on a discrete card can take that long on an
//! integrated one, so the size of a submission is never fixed: it starts small, is measured, and
//! grows toward a target duration far below the watchdog.

use std::time::Duration;

/// Start-up budget and timing targets for compute work, by kind of adapter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GpuBudget {
    /// Work units of the first compositing submission (one unit is one layer operation on one
    /// 64 x 64 tile).
    pub first_units: u64,
    /// Wanted duration of one submission.
    pub target: Duration,
    /// A submission longer than this shrinks the next one at once.
    pub max: Duration,
    /// Largest growth from one submission to the next.
    pub growth: u64,
}

impl GpuBudget {
    /// Discrete cards: start at a size they finish in well under a millisecond.
    pub const DISCRETE: Self = Self { first_units: 2048, target: Duration::from_millis(100), max: Duration::from_millis(250), growth: 4 };
    /// Integrated, software and unknown adapters: a much smaller first step, a shorter target
    /// and slower growth, so even a very slow adapter stays far from the two-second watchdog.
    pub const CAUTIOUS: Self = Self { first_units: 64, target: Duration::from_millis(40), max: Duration::from_millis(120), growth: 2 };

    /// `device_type` is the `Debug` text of `wgpu::DeviceType` (as stored in `GpuCaps`).
    pub fn for_device_type(device_type: &str) -> Self {
        if device_type == "DiscreteGpu" { Self::DISCRETE } else { Self::CAUTIOUS }
    }

    pub fn describe(&self) -> String {
        format!("first compute submission {} units, target {} ms, limit {} ms, growth x{}", self.first_units, self.target.as_millis(), self.max.as_millis(), self.growth)
    }
}

/// Video memory budget, by kind of adapter (issue #119: an integrated AMD GPU with 512 MB of
/// dedicated memory was reset by the driver during start-up).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemoryBudget {
    /// 4x multisampling of the viewport (four times the colour and depth targets).
    pub msaa: bool,
    /// Upper limit of the GPU compositing tile pool.
    pub composite_pool_mb: u64,
    /// Widest environment map level uploaded for the viewport.
    pub environment_width: u32,
}

impl MemoryBudget {
    pub const ROOMY: Self = Self { msaa: true, composite_pool_mb: 2048, environment_width: u32::MAX };
    /// Integrated GPUs share the system memory and often have very little of their own.
    pub const TIGHT: Self = Self { msaa: false, composite_pool_mb: 128, environment_width: 1024 };

    /// `device_type` and `backend` are the `Debug` texts stored in `GpuCaps`. wgpu does not
    /// report the amount of video memory, so the kind of adapter decides. Apple's integrated
    /// GPUs (Metal) have plenty of unified memory and keep the full budget;
    /// `SUBCRAFT_FULL_GPU_MEMORY=1` gives it to any adapter.
    pub fn for_adapter(device_type: &str, backend: &str) -> Self {
        Self::choose(device_type, backend, std::env::var_os("SUBCRAFT_FULL_GPU_MEMORY").is_some())
    }

    pub fn choose(device_type: &str, backend: &str, full: bool) -> Self {
        if device_type == "IntegratedGpu" && backend != "Metal" && !full { Self::TIGHT } else { Self::ROOMY }
    }

    pub fn is_tight(&self) -> bool {
        *self == Self::TIGHT
    }

    pub fn describe(&self) -> String {
        if self.is_tight() {
            format!("low video memory budget: 4x anti-aliasing off, GPU compositing pool up to {} MB, environment maps up to {} px wide", self.composite_pool_mb, self.environment_width)
        } else {
            format!("full video memory budget: GPU compositing pool up to {} MB", self.composite_pool_mb)
        }
    }

    /// How many of the widest levels of an environment map (widths given, widest first) to
    /// leave out. At least four levels stay, so rough reflections still have their blur chain.
    pub fn environment_levels_skipped(&self, widths: &[u32]) -> usize {
        let fitting = widths.iter().take_while(|&&w| w > self.environment_width).count();
        fitting.min(widths.len().saturating_sub(4))
    }
}

/// Adaptive size of one submission, in work units.
#[derive(Clone, Debug)]
pub struct DispatchPace {
    budget: GpuBudget,
    units: u64,
}

impl DispatchPace {
    pub fn new(budget: GpuBudget) -> Self {
        Self { budget, units: budget.first_units.max(1) }
    }

    /// Work units the next submission may hold.
    pub fn units(&self) -> u64 {
        self.units
    }

    /// Record a finished submission of `units` that took `took`.
    pub fn record(&mut self, units: u64, took: Duration) {
        if units == 0 {
            return;
        }
        let per_unit = took.as_secs_f64().max(1e-6) / units as f64;
        let fit = (self.budget.target.as_secs_f64() / per_unit).clamp(1.0, 1e15) as u64;
        if took > self.budget.max {
            // Too long: go straight to the size that fits the target.
            self.units = fit.min(self.units).max(1);
        } else if took > self.budget.target {
            self.units = fit.max(1);
        } else {
            // A small submission says little about speed (fixed overhead dominates), so it can
            // only raise the size, by at most `growth` times what was measured.
            self.units = self.units.max(fit.min(units.saturating_mul(self.budget.growth)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrated_adapters_get_the_low_memory_budget() {
        assert_eq!(MemoryBudget::choose("IntegratedGpu", "Dx12", false), MemoryBudget::TIGHT);
        assert_eq!(MemoryBudget::choose("IntegratedGpu", "Vulkan", false), MemoryBudget::TIGHT);
        assert_eq!(MemoryBudget::choose("IntegratedGpu", "Dx12", true), MemoryBudget::ROOMY, "the override");
        assert_eq!(MemoryBudget::choose("IntegratedGpu", "Metal", false), MemoryBudget::ROOMY);
        for kind in ["DiscreteGpu", "Cpu", "VirtualGpu", "Other", ""] {
            assert_eq!(MemoryBudget::choose(kind, "Dx12", false), MemoryBudget::ROOMY, "{kind}");
        }
        let (t, r) = (MemoryBudget::TIGHT, MemoryBudget::ROOMY);
        assert!(!t.msaa && r.msaa && t.composite_pool_mb < r.composite_pool_mb && t.environment_width < r.environment_width);
        assert!(t.is_tight() && !r.is_tight());
        assert!(t.describe().contains("anti-aliasing off") && t.describe().contains("128 MB") && t.describe().contains("1024 px"));
        // A 4096 px map with 8 levels loses its two widest levels; a small one loses none; a
        // short chain keeps four levels even when they are wide.
        assert_eq!(t.environment_levels_skipped(&[4096, 2048, 1024, 512, 256, 128, 64, 32]), 2);
        assert_eq!(t.environment_levels_skipped(&[1024, 512, 256, 128, 64]), 0);
        assert_eq!(t.environment_levels_skipped(&[8192, 4096, 2048, 1024, 512]), 1);
        assert_eq!(t.environment_levels_skipped(&[]), 0);
        assert_eq!(r.environment_levels_skipped(&[8192, 4096, 2048, 1024, 512, 256]), 0);
    }

    #[test]
    fn integrated_and_unknown_adapters_get_the_cautious_budget() {
        assert_eq!(GpuBudget::for_device_type("DiscreteGpu"), GpuBudget::DISCRETE);
        for kind in ["IntegratedGpu", "Cpu", "VirtualGpu", "Other", ""] {
            assert_eq!(GpuBudget::for_device_type(kind), GpuBudget::CAUTIOUS, "{kind}");
        }
        let (d, c) = (GpuBudget::DISCRETE, GpuBudget::CAUTIOUS);
        assert!(c.first_units < d.first_units && c.target < d.target && c.max < d.max && c.growth < d.growth);
        // Both stay far below the two-second watchdog, even after one full growth step on top
        // of a submission that only just passed as "not too long".
        for b in [d, c] {
            assert!(b.max.as_secs_f64() * b.growth as f64 <= 1.0, "{b:?}");
        }
        assert!(c.describe().contains("64 units") && c.describe().contains("40 ms"));
    }

    #[test]
    fn a_fast_adapter_grows_step_by_step_and_never_past_the_growth_factor() {
        let mut p = DispatchPace::new(GpuBudget::CAUTIOUS);
        assert_eq!(p.units(), 64);
        let mut sizes = Vec::new();
        for _ in 0..6 {
            let n = p.units();
            p.record(n, Duration::from_micros(200)); // far faster than the target
            sizes.push(p.units());
        }
        assert_eq!(sizes, vec![128, 256, 512, 1024, 2048, 4096]);
    }

    #[test]
    fn a_slow_submission_shrinks_at_once_to_fit_the_target() {
        let mut p = DispatchPace::new(GpuBudget::DISCRETE);
        // 2048 units took 1 s: 100 ms fits about 204 units.
        p.record(2048, Duration::from_secs(1));
        assert!((200..=210).contains(&p.units()), "{}", p.units());
        // Slightly over the target (not over the limit): settle on the fitting size.
        let mut p = DispatchPace::new(GpuBudget::DISCRETE);
        p.record(2048, Duration::from_millis(200));
        assert_eq!(p.units(), 1024);
        // Hopelessly slow: never below one unit.
        p.record(1, Duration::from_secs(5));
        assert_eq!(p.units(), 1);
    }

    #[test]
    fn a_small_leftover_submission_does_not_shrink_the_learned_size() {
        let mut p = DispatchPace::new(GpuBudget::DISCRETE);
        for _ in 0..8 {
            let n = p.units();
            p.record(n, Duration::from_millis(1));
        }
        let learned = p.units();
        assert!(learned > 100_000, "{learned}");
        // The last few tiles of a batch: tiny, dominated by fixed overhead.
        p.record(8, Duration::from_millis(1));
        assert_eq!(p.units(), learned);
        p.record(0, Duration::from_secs(9));
        assert_eq!(p.units(), learned);
    }

    /// A simulated adapter with a fixed speed: no submission may come near the watchdog.
    #[test]
    fn no_submission_reaches_the_watchdog_on_a_very_slow_adapter() {
        for budget in [GpuBudget::CAUTIOUS, GpuBudget::DISCRETE] {
            // 5 ms of fixed overhead and 0.2 ms per unit: 26 000 units (a 2048 px set with
            // several layers) in one go would take over five seconds.
            let cost = |units: u64| Duration::from_micros(5_000 + units * 200);
            let mut p = DispatchPace::new(budget);
            let (mut left, mut longest) = (26_000u64, Duration::ZERO);
            while left > 0 {
                let n = p.units().min(left);
                let took = cost(n);
                longest = longest.max(took);
                p.record(n, took);
                left -= n;
            }
            assert!(longest < Duration::from_millis(600), "{budget:?}: longest submission {longest:?}");
        }
    }
}
