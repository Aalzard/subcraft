use egui::{Event, Pos2, Rect};
use subcraft_app::Session;
use subcraft_ui::{panels::texture_sets, state::UiState, theme};

fn frame(ctx: &egui::Context, state: &mut UiState, width: f32, height: f32, events: Vec<Event>) -> (egui::FullOutput, Rect) {
    let mut bounds = Rect::NOTHING;
    let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(width,height))), events, ..Default::default() }, |ui| {
        texture_sets::show(state, ui); bounds = ui.min_rect();
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
fn bottom_strip_fits_controls_horizontally_and_sidebar_stays_vertical() {
    for (width,height) in [(680.,100.),(900.,140.),(1280.,180.),(300.,600.)] {
        let (mut state,ctx)=setup(); let before=state.session.document().clone();
        let mut result=frame(&ctx,&mut state,width,height,vec![]);
        for _ in 0..2 {result=frame(&ctx,&mut state,width,height,vec![]);}
        let (out,bounds)=result;
        assert!(bounds.right()<=width+0.5,"horizontal overflow: {bounds:?}");
        let names=text(&out,"Body"); let res=text(&out,"RESOLUTION"); let maps=text(&out,"MESH MAPS"); let albedo=text(&out,"Albedo");
        if width>=660. {
            assert!(res.left()>names.right() && maps.left()>res.right(),"independent horizontal sections");
            assert!(bounds.bottom()<=height+0.5,"bottom panel should not need vertical scrolling: {bounds:?}");
            for shape in &out.shapes {
                if let egui::Shape::Text(t)=&shape.shape {
                    assert!(shape.clip_rect.contains_rect(Rect::from_min_size(t.pos,t.galley.size())),"clipped control: {}",t.galley.text());
                }
            }
        } else {assert!(res.top()>names.bottom() && maps.top()>albedo.bottom());}
        assert!(albedo.top()>res.top()); assert_eq!(state.session.document(),&before);
    }
}

#[test]
fn compact_texture_set_selection_and_bake_button_dispatch_correctly() {
    let (mut state,ctx)=setup();
    for _ in 0..3 {frame(&ctx,&mut state,900.,140.,vec![]);}
    let trim=text(&frame(&ctx,&mut state,900.,140.,vec![]).0,"Trim").center();
    let click=|state:&mut UiState,pos| {for pressed in [true,false] {
        frame(&ctx,state,900.,140.,vec![Event::PointerMoved(pos),Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()}]);
    }};
    click(&mut state,trim); assert_eq!(state.session.selected_texture_set().name,"Trim");
    let out=frame(&ctx,&mut state,900.,140.,vec![]).0;
    let bake=text(&out,&format!("{} Bake…",subcraft_ui::icons::MAGIC_WAND)).center(); click(&mut state,bake);
    assert!(state.bake.open);
}
