//! An occasional invitation to the SubCraft Discord server: a small card in the bottom-right
//! corner, never on the first launches, never twice in a row, and never again once answered.
use crate::state::UiState;
use serde::{Deserialize, Serialize};

pub const DISCORD_INVITE: &str = "https://discord.gg/9A7ThB5kE";
/// The first launch that may show the invitation.
const FIRST_LAUNCH: u32 = 3;
/// Launches between two invitations when the user picks "Later".
const EVERY_LAUNCHES: u32 = 10;
/// Seconds into a session before the card appears, so it never interrupts opening a file.
pub const DELAY_SECONDS: f64 = 90.0;

/// Kept in the preferences file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommunityPrefs {
    /// Interactive launches so far.
    pub launches: u32,
    /// The launch the invitation was last shown on (0 = never).
    pub invited_at: u32,
    /// Joined or chose "Don't show again".
    pub done: bool,
}

impl CommunityPrefs {
    /// Count this launch and say whether it is one to invite on.
    pub fn launch(&mut self) -> bool {
        self.launches = self.launches.saturating_add(1);
        !self.done && self.launches >= FIRST_LAUNCH
            && (self.invited_at == 0 || self.launches - self.invited_at.min(self.launches) >= EVERY_LAUNCHES)
    }
}

/// This session's invitation.
#[derive(Default)]
pub struct CommunityUi {
    /// Show the card once the session is this many seconds old.
    pub due_at: Option<f64>,
}

/// Something else wants the user's attention first.
fn busy(state: &UiState) -> bool {
    state.welcome.open || state.welcome.guide.is_some() || state.update_notice
        || !state.recovery.crashed.is_empty() || !state.bug_report.crashes.is_empty()
        || state.updater.as_ref().is_some_and(|u| u.open) || state.bake.job.is_some()
}

pub fn show(state: &mut UiState, ctx: &egui::Context, now: f64) {
    use crate::theme::{self, ACCENT, BG_2, TEXT, TEXT_DIM};
    use egui::{Order, RichText, Stroke};
    let Some(due) = state.community.due_at else { return };
    if now < due {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64((due - now).max(0.5)));
        return;
    }
    if busy(state) || ctx.input(|i| i.pointer.any_down()) { return; }
    // Counted as shown from the first frame, so a crash or a quit doesn't bring it straight back.
    let prefs = &mut state.recovery.prefs.community;
    prefs.invited_at = prefs.launches;
    const WIDTH: f32 = 300.0;
    let screen = ctx.content_rect();
    let pos = egui::pos2(screen.right() - WIDTH - 16.0, screen.bottom() - 48.0);
    let mut answer = None;
    egui::Area::new(egui::Id::new("discord_invite")).order(Order::Foreground).pivot(egui::Align2::LEFT_BOTTOM).fixed_pos(pos).show(ctx, |ui| {
        egui::Frame::new().fill(BG_2()).stroke(Stroke::new(1.0, ACCENT())).corner_radius(6.0).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
            ui.set_width(WIDTH - 26.0);
            ui.label(RichText::new("Join the SubCraft Discord").font(theme::font_strong(13.0)).color(TEXT()));
            ui.add_space(2.0);
            ui.label(RichText::new("Share your work, ask questions, suggest features and hear about new versions first.").color(TEXT_DIM()));
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if crate::widgets::primary_button(ui, "Join Discord").on_hover_text(DISCORD_INVITE).clicked() { answer = Some(Answer::Join); }
                if crate::widgets::secondary_button(ui, "Later").clicked() { answer = Some(Answer::Later); }
                if ui.small_button("Don't show again").clicked() { answer = Some(Answer::Never); }
            });
        });
    });
    if let Some(answer) = answer {
        state.community.due_at = None;
        let prefs = &mut state.recovery.prefs.community;
        match answer {
            Answer::Join => { prefs.done = true; ctx.open_url(egui::OpenUrl::new_tab(DISCORD_INVITE)); }
            Answer::Never => prefs.done = true,
            Answer::Later => {}
        }
    }
}

enum Answer { Join, Later, Never }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invites_from_the_third_launch_then_every_tenth_until_answered() {
        let mut p = CommunityPrefs::default();
        assert!(!p.launch() && !p.launch(), "not on the first two launches");
        assert!(p.launch(), "third launch");
        p.invited_at = p.launches; // shown, user picked Later
        for _ in 0..9 { assert!(!p.launch()); }
        assert!(p.launch(), "ten launches later");
        p.done = true;
        for _ in 0..30 { assert!(!p.launch(), "never after Join or Don't show again"); }
    }

    #[test]
    fn a_launch_not_shown_on_keeps_it_due() {
        // Due but the session ended before the card appeared: still due next time.
        let mut p = CommunityPrefs { launches: 2, ..Default::default() };
        assert!(p.launch() && p.launch());
    }

    #[test]
    fn older_preferences_start_counting_from_zero() {
        let p: crate::recovery::Preferences = serde_json::from_str(r#"{"show_welcome":false}"#).unwrap();
        assert_eq!(p.community, CommunityPrefs::default());
    }
}
