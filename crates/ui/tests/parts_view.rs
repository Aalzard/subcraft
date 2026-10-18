//! GPU regressions for visibility changes and sampling that evaluates pending composites.
use std::sync::Arc;
use subcraft_app::{PartAction, PartKey, Session};
use subcraft_core::{ChannelKind, ChannelValue, PartKind};
use subcraft_mesh::primitives;
use subcraft_render::{egui_wgpu, headless, wgpu_api as wgpu};
use subcraft_ui::state::UiState;

fn ui_state() -> Option<UiState> {
    let (adapter, device, queue) = headless::device()?;
    let target_format = wgpu::TextureFormat::Rgba8Unorm;
    let renderer = egui_wgpu::Renderer::new(&device, target_format, Default::default());
    let rs = egui_wgpu::RenderState {
        adapter, available_adapters: Vec::new(), device, queue, target_format,
        instance: wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle()),
        renderer: Arc::new(egui::epaint::mutex::RwLock::new(renderer)),
        surface_config: egui_wgpu::SurfaceConfig::LOW_LATENCY,
    };
    Some(UiState::new(Session::new(primitives::sample_crate(), 128), Some(rs)))
}
fn render(ui: &mut UiState) -> Vec<u8> {
    // Native viewports now publish composites asynchronously. Drain through actual frame
    // syncs (not Session::update_composites, which would consume upload notifications).
    let start = std::time::Instant::now();
    loop {
        ui.begin_frame();
        ui.sync_gpu();
        if !ui.session.composites_pending() { break; }
        assert!(start.elapsed() < std::time::Duration::from_secs(30), "viewport composite converges");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let params = ui.camera.view_params([128, 128]);
    let rs = ui.render_state.as_ref().unwrap();
    ui.renderer.as_mut().unwrap().render(&rs.device, &rs.queue, [128, 128], &params).read_rgba8(&rs.device, &rs.queue)
}
fn visible_pixels(image: &[u8]) -> usize {
    image.as_chunks::<4>().0.iter().filter(|p| p[0] > 90 || p[1] > 90 || p[2] > 90).count()
}

#[test]
fn isolate_hide_and_restore_reupload_materials_and_render_real_geometry() {
    let Some(mut ui) = ui_state() else { eprintln!("no adapter: skipping"); return };
    let before = render(&mut ui);
    let target = ui.session.target_for_part(PartKey { kind: PartKind::Connected, id: 0 }).unwrap();
    ui.session.apply_part_action(target, PartAction::Isolate).unwrap();
    let isolated = render(&mut ui);
    assert!(visible_pixels(&isolated) > 1000, "isolated body still has its GPU material");
    assert_ne!(isolated, before);
    ui.session.show_all_parts(); assert_eq!(render(&mut ui), before);
    ui.session.apply_part_action(target, PartAction::Hide).unwrap();
    let hidden = render(&mut ui);
    assert!(visible_pixels(&hidden) > 100, "trim still renders after hiding the body");
    assert_ne!(hidden, before);
    ui.session.show_all_parts(); assert_eq!(render(&mut ui), before);
}

#[test]
fn visibility_refresh_keeps_baked_ambient_occlusion_uploaded() {
    let Some(mut ui) = ui_state() else { eprintln!("no adapter: skipping"); return };
    let target = ui.session.target_for_part(PartKey { kind: PartKind::Connected, id: 0 }).unwrap();
    ui.session.set_mesh_maps(target.texture_set, [(subcraft_core::MeshMapKind::AmbientOcclusion,
        subcraft_image::TiledImage::new(128, 128, 1, [0.0; 4]))].into_iter().collect());
    let dark = render(&mut ui);
    // Hide and restore must refresh both channel textures and the AO fallback.
    ui.session.apply_part_action(target, PartAction::Hide).unwrap(); render(&mut ui);
    ui.session.show_all_parts(); assert_eq!(render(&mut ui), dark);
}

#[test]
fn eyedropper_cannot_consume_a_fill_before_it_reaches_the_gpu() {
    let Some(mut ui) = ui_state() else { eprintln!("no adapter: skipping"); return };
    let before = render(&mut ui);
    let target = ui.session.target_for_part(PartKey { kind: PartKind::Connected, id: 0 }).unwrap();
    ui.session.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(0.8, 0.02, 0.02));
    ui.session.apply_part_action(target, PartAction::Fill).unwrap();
    subcraft_ui::panels::parts::apply(&mut ui, target, PartAction::PickColor, 0.0);
    let after = render(&mut ui);
    let changed = before.iter().zip(&after).filter(|(a, b)| a.abs_diff(**b) > 8).count();
    assert!(changed > 1000, "sampled fill reaches the viewport");
}

#[test]
fn synchronous_export_cannot_leave_a_stale_viewport_composite() {
    let Some(mut ui) = ui_state() else { eprintln!("no adapter: skipping"); return };
    let before = render(&mut ui);
    let target = ui.session.target_for_part(PartKey { kind: PartKind::Connected, id: 0 }).unwrap();
    ui.session.brush.material.set(ChannelKind::BaseColor, ChannelValue::rgb(0.02, 0.8, 0.02));
    ui.session.apply_part_action(target, PartAction::Fill).unwrap();
    ui.session.export_archive(&subcraft_app::ExportSettings::default()).unwrap();
    let after = render(&mut ui);
    let changed = before.iter().zip(&after).filter(|(a, b)| a.abs_diff(**b) > 8).count();
    assert!(changed > 1000, "export's synchronous composite still reaches the GPU");
    ui.gpu_dirty = true;
    assert!(render(&mut ui) == after, "incremental viewport equals a forced complete upload");
}
