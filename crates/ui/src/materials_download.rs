//! Assets panel > Materials > Download SubCraft Materials: find the newest pack release, let the
//! user pick 1K or 2K (with their sizes) and confirm, show the download, then use the unpacked
//! pack as the Settings > Libraries pack folder.

use egui::RichText;
use subcraft_app::materials_download::Phase;

use crate::i18n::tr;
use crate::state::UiState;
use crate::theme;

/// A size in bytes for people ("312 MB", "1.4 GB").
pub fn format_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let mb = bytes as f64 / MB;
    if mb >= 1024.0 { format!("{:.1} GB", mb / 1024.0) } else if mb >= 10.0 { format!("{mb:.0} MB") } else { format!("{mb:.1} MB") }
}

/// Open the window and look for the newest pack.
pub fn open(state: &mut UiState) {
    state.materials_download_open = true;
    if !state.materials_downloader.busy() {
        state.materials_downloader.check();
    }
}

/// A finished download: the unpacked folder becomes the pack folder.
pub fn use_downloaded(state: &mut UiState, folder: &std::path::Path, now: f64) {
    state.recovery.prefs.materials_pack = folder.display().to_string();
    state.reload_materials_pack();
    let n = state.materials_pack.len();
    state.set_status(format!("SubCraft Materials: {n} materials ready in the Assets panel"), now);
}

pub fn window(state: &mut UiState, ctx: &egui::Context) {
    state.materials_downloader.poll();
    if let Phase::Done(folder) = state.materials_downloader.phase.clone() {
        use_downloaded(state, &folder, ctx.input(|i| i.time));
        state.materials_downloader.phase = Phase::Idle;
        state.materials_download_open = false;
    }
    if !state.materials_download_open {
        return;
    }
    if state.materials_downloader.busy() {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
    let mut open = true;
    let mut start = None;
    let mut retry = false;
    crate::widgets::window(ctx, RichText::new(tr("SubCraft Materials")).font(theme::font_strong(14.0))).id(egui::Id::new("materials_download")).open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.set_width(380.0);
        match &state.materials_downloader.phase {
            Phase::Idle | Phase::Checking => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr("Looking for the newest materials pack…"));
                });
            }
            Phase::NoRelease => {
                ui.label(tr("The SubCraft Materials pack is not published yet."));
            }
            Phase::Available(release) => {
                ui.label(format!("{} ({})", tr("A pack of tileable PBR materials, downloaded into SubCraft's data folder and listed in Assets > Materials."), release.tag));
                ui.add_space(6.0);
                let choice = &mut state.materials_download_choice;
                *choice = (*choice).min(release.options.len().saturating_sub(1));
                for (i, o) in release.options.iter().enumerate() {
                    ui.radio_value(choice, i, format!("{} · {}", o.resolution, format_size(o.zip.size)));
                }
                ui.add_space(6.0);
                if crate::widgets::primary_button(ui, tr("Download")).clicked() {
                    start = Some((release.clone(), release.options[*choice].clone()));
                }
            }
            Phase::Downloading { bytes, total } => {
                let f = if *total > 0 { *bytes as f32 / *total as f32 } else { 0.0 };
                ui.label(format!("{} {} / {}", tr("Downloading"), format_size(*bytes), format_size(*total)));
                ui.add(egui::ProgressBar::new(f).show_percentage());
            }
            Phase::Extracting => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr("Unpacking the materials…"));
                });
            }
            Phase::Done(_) => {}
            Phase::Error(e) => {
                ui.label(RichText::new(e).color(theme::on_panel(egui::Color32::from_rgb(226, 92, 92))));
                retry = crate::widgets::secondary_button(ui, tr("Try again")).clicked();
            }
        }
    });
    if let Some((release, option)) = start {
        state.materials_downloader.download(&release, &option);
    }
    if retry {
        state.materials_downloader.check();
    }
    // Closing the window does not stop a running download; it finishes in the background.
    state.materials_download_open = open;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_a_download_dialog() {
        assert_eq!(format_size(5 * 1024 * 1024 / 2), "2.5 MB");
        assert_eq!(format_size(312 * 1024 * 1024), "312 MB");
        assert_eq!(format_size(1536 * 1024 * 1024), "1.5 GB");
    }

    #[test]
    fn a_downloaded_pack_becomes_the_pack_folder() {
        let mut s = UiState::new(subcraft_app::Session::new(subcraft_mesh::primitives::cube(2.0), 128), None);
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/tests/fixtures/materials_pack");
        use_downloaded(&mut s, &fixtures, 0.0);
        assert_eq!(s.recovery.prefs.materials_pack, fixtures.display().to_string());
        assert_eq!(s.materials_pack.len(), 2, "its materials are listed right away");
    }
}
