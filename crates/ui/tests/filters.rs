use egui::{Pos2,Rect};
use subcraft_app::{Command,NewLayer,Session};
use subcraft_core::ChannelKind;
use subcraft_doc::{ContentEffectKind,Filter,MaskEffectKind};
use subcraft_ui::{panels::{effects,filter},state::UiState,theme};

fn labels(out:&egui::FullOutput)->Vec<String> {
    out.shapes.iter().filter_map(|s|match &s.shape {egui::Shape::Text(t)=>Some(t.galley.text().to_string()),_=>None}).collect()
}

fn frame(ctx:&egui::Context,input:egui::RawInput,mut draw:impl FnMut(&mut egui::Ui))->egui::FullOutput {
    let mut out=ctx.run_ui(input,|ui|draw(ui));out.textures_delta.clear();out
}

#[test]
fn selected_content_and_mask_filters_show_their_own_controls() {
    let mut session=Session::new(subcraft_mesh::primitives::cube(2.0),64);
    session.execute(Command::AddLayer(NewLayer::Fill)).unwrap(); let layer=session.selection().layer.unwrap();
    let mut state=UiState::new(session,None);let ctx=egui::Context::default();theme::apply(&ctx);
    for (filter,expected) in [(Filter::ALL[0],vec!["Contrast","Pivot"]),(Filter::ALL[1],vec!["Hue","Saturation","Lightness"]),
        (Filter::ALL[2],vec!["Cutoff"]),(Filter::ALL[3],vec!["Minimum","Maximum"])] {
        state.session.execute(Command::AddContentEffect {layer,kind:ContentEffectKind::Filter {channel:ChannelKind::BaseColor,filter}}).unwrap();
        let mut out=ctx.run_ui(egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(480.0,1200.0))),..Default::default()},
            |ui| { assert!(effects::properties(&mut state,ui)); });
        out.textures_delta.clear(); let text=labels(&out);
        for name in expected { assert!(text.iter().any(|s|s==name),"missing {name}: {text:?}"); }
    }
    state.session.execute(Command::AddMask {layer,white:false}).unwrap();
    state.session.execute(Command::AddMaskEffect {layer,kind:MaskEffectKind::Filter(Filter::Threshold {cutoff:0.45})}).unwrap();
    let out=frame(&ctx,egui::RawInput::default(),|ui| { assert!(effects::properties(&mut state,ui)); });
    let text=labels(&out);assert!(text.iter().any(|s|s=="Cutoff"));assert!(!text.iter().any(|s|s=="Hue"));
}

#[test]
fn mask_filter_picker_offers_scalar_operations_and_excludes_hsl() {
    let ctx=egui::Context::default();theme::apply(&ctx);let mut chosen=Filter::ALL[0];
    let input=egui::RawInput {screen_rect:Some(Rect::from_min_size(Pos2::ZERO,egui::vec2(480.0,1000.0))),..Default::default()};
    frame(&ctx,input.clone(),|ui|{filter::editor(ui,&mut chosen,None);});
    let out=frame(&ctx,input.clone(),|ui|{filter::editor(ui,&mut chosen,None);});
    let text=labels(&out);assert!(text.iter().any(|s|s=="Contrast"));
    // Open the real filter dropdown and inspect offered operations.
    let pos=out.shapes.iter().find_map(|s|match &s.shape {egui::Shape::Text(t) if t.galley.text()=="Contrast"=>Some(t.pos+t.galley.size()*0.5),_=>None}).unwrap();
    let button=|pressed|egui::Event::PointerButton {pos,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()};
    let mut press=input.clone();press.events=vec![egui::Event::PointerMoved(pos),button(true)];frame(&ctx,press,|ui|{filter::editor(ui,&mut chosen,None);});
    let mut release=input.clone();release.events=vec![button(false)];
    frame(&ctx,release,|ui|{filter::editor(ui,&mut chosen,None);});
    let out=frame(&ctx,input,|ui|{filter::editor(ui,&mut chosen,None);});
    let text=labels(&out);assert!(text.iter().any(|s|s=="Threshold"),"{text:?}");assert!(text.iter().any(|s|s=="Clamp"));assert!(!text.iter().any(|s|s=="HSL"));
}
