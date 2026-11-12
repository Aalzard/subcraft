use egui::{Event, Pos2, Rect};
use subcraft_app::Session;
use subcraft_ui::{panels::texture_sets, state::UiState, theme};

fn frame(ctx: &egui::Context, state: &mut UiState, f: fn(&mut UiState, &mut egui::Ui), width: f32, height: f32, events: Vec<Event>) -> (egui::FullOutput, Rect) {
    let mut bounds = Rect::NOTHING;
    let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width,height))), events, ..Default::default() }, |ui| {
        f(state, ui); bounds = ui.min_rect();
    });
    out.textures_delta.clear(); (out,bounds)
}
fn text(out: &egui::FullOutput, label: &str) -> Rect {
    out.shapes.iter().find_map(|s| match &s.shape {
        egui::Shape::Text(t) if t.galley.text() == label => Some(Rect::from_min_size(t.pos,t.galley.size())), _ => None
    }).unwrap_or_else(|| panic!("missing {label}"))
}
fn setup() -> (UiState,egui::Context) {
    let ctx = egui::Context::default(); theme::apply(&ctx);
    (UiState::new(Session::new(subcraft_mesh::primitives::sample_crate(),128),None),ctx)
}

#[test]
fn panels_never_overflow_horizontally_and_leave_the_document_alone() {
    for (width,height) in [(300.,600.),(420.,900.),(600.,300.)] {
        for (which, f) in [("list", texture_sets::list as fn(&mut UiState,&mut egui::Ui)), ("settings", texture_sets::settings as fn(&mut UiState,&mut egui::Ui))] {
            let (mut state,ctx)=setup(); let before=state.session.document().clone();
            let mut result=frame(&ctx,&mut state,f,width,height,vec![]);
            for _ in 0..2 {result=frame(&ctx,&mut state,f,width,height,vec![]);}
            assert!(result.1.right()<=width+4.0,/* the vertical scroll bar may overhang by ~3px at 300 */"{which}: horizontal overflow at {width}: {:?}",result.1);
            assert_eq!(state.session.document(),&before);
        }
    }
}

#[test]
fn list_selection_and_settings_bake_button_dispatch_correctly() {
    let (mut state,ctx)=setup();
    let click=|state:&mut UiState,f:fn(&mut UiState,&mut egui::Ui),pos| {for pressed in [true,false] {
        frame(&ctx,state,f,420.,900.,vec![Event::PointerMoved(pos),Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}]);
    }};
    for _ in 0..3 {frame(&ctx,&mut state,texture_sets::list,420.,900.,vec![]);}
    let trim=text(&frame(&ctx,&mut state,texture_sets::list,420.,900.,vec![]).0,"Trim").center();
    click(&mut state,texture_sets::list,trim); assert_eq!(state.session.selected_texture_set().name,"Trim");
    for _ in 0..3 {frame(&ctx,&mut state,texture_sets::settings,420.,900.,vec![]);}
    let out=frame(&ctx,&mut state,texture_sets::settings,420.,900.,vec![]).0;
    let bake=text(&out,"Bake Mesh Maps").center(); click(&mut state,texture_sets::settings,bake);
    assert!(state.bake.open);
}
