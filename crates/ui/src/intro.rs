//! Startup intro: the logo fades in over half a second on the app's background, then the overlay
//! fades away and reveals the workspace (which loads underneath meanwhile). Skipped for scripted
//! screenshots and tests.
use crate::theme;

const FADE_IN: f64 = 0.5;
const FADE_OUT: f64 = 0.2;

#[derive(Default)]
pub struct Intro {
    start: Option<f64>,
    logo: Option<egui::TextureHandle>,
}

impl Intro {
    /// Opacity of the overlay and of the logo `t` seconds after the first frame; `None` once
    /// the intro is over.
    pub fn alphas(t: f64) -> Option<(f32, f32)> {
        if t >= FADE_IN + FADE_OUT {
            return None;
        }
        let logo = (t / FADE_IN).clamp(0.0, 1.0);
        let logo = (logo * logo * (3.0 - 2.0 * logo)) as f32; // smoothstep
        let overlay = if t <= FADE_IN { 1.0 } else { (1.0 - (t - FADE_IN) / FADE_OUT) as f32 };
        Some((overlay, logo * overlay))
    }
}

/// Draw the intro over everything. Returns false (and drops it) when it has finished.
pub fn show(ctx: &egui::Context, intro: &mut Option<Intro>) {
    let Some(state) = intro.as_mut() else { return };
    let now = ctx.input(|i| i.time);
    let start = *state.start.get_or_insert(now);
    let Some((overlay, logo)) = Intro::alphas(now - start) else {
        *intro = None;
        return;
    };
    let tex = state
        .logo
        .get_or_insert_with(|| {
            let img = image::load_from_memory_with_format(include_bytes!("../assets/icon.png"), image::ImageFormat::Png)
                .expect("embedded icon is a valid PNG")
                .into_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            ctx.load_texture("intro_logo", egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()), egui::TextureOptions::LINEAR)
        })
        .clone();
    let screen = ctx.content_rect();
    egui::Area::new(egui::Id::new("intro"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            // Swallow clicks while the overlay is up.
            let (rect, _) = ui.allocate_exact_size(screen.size(), egui::Sense::click_and_drag());
            let p = ui.painter();
            p.rect_filled(rect, 0.0, theme::BG_0().gamma_multiply(overlay));
            let side = 112.0;
            let center = rect.center() - egui::vec2(0.0, 18.0);
            let logo_rect = egui::Rect::from_center_size(center, egui::vec2(side, side));
            p.image(tex.id(), logo_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE.gamma_multiply(logo));
            p.text(
                egui::pos2(center.x, logo_rect.bottom() + 22.0),
                egui::Align2::CENTER_CENTER,
                "SubCraft",
                theme::font_strong(22.0),
                theme::TEXT().gamma_multiply(logo),
            );
        });
    ctx.request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_fades_in_over_half_a_second_then_the_overlay_lifts() {
        assert_eq!(Intro::alphas(0.0), Some((1.0, 0.0)));
        let (o, l) = Intro::alphas(0.25).unwrap();
        assert_eq!(o, 1.0);
        assert!((l - 0.5).abs() < 0.01, "halfway in at 0.25 s");
        assert_eq!(Intro::alphas(0.5), Some((1.0, 1.0)), "fully in at 0.5 s");
        let (o, _) = Intro::alphas(0.6).unwrap();
        assert!(o < 1.0 && o > 0.0, "then the overlay fades");
        assert_eq!(Intro::alphas(0.7), None, "and is gone");
    }
}
