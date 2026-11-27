//! Painter's searchable mini shelf (issue #760): a button showing the current choice that opens a
//! list with a search field. Typing filters the list; Enter takes the first match.
use egui::Ui;

/// Whether `label` contains every word of `query` (case-insensitive). An empty query matches all.
pub fn matches(query: &str, label: &str) -> bool {
    let label = label.to_lowercase();
    query.split_whitespace().all(|w| label.contains(&w.to_lowercase()))
}

/// The entries of `items` whose label matches `query`, in list order.
pub fn filter<'a, T>(query: &str, items: &'a [(String, T)]) -> Vec<&'a (String, T)> {
    items.iter().filter(|(label, _)| matches(query, label)).collect()
}

/// A dropdown button labelled `current` that opens `items` with a search field (hint `hint`).
/// Returns the picked value, never the current one.
pub fn picker<T: Clone>(ui: &mut Ui, id_salt: &str, current: &str, hint: &str, items: &[(String, T)]) -> Option<T> {
    let button = ui.add(egui::Button::new(format!("{current}  {}", crate::icons::CARET_DOWN)).truncate()).on_hover_text("Pick another (type to search)");
    let id = ui.id().with(id_salt);
    let query_id = id.with("query");
    let mut picked = None;
    egui::Popup::menu(&button).id(id.with("popup")).show(|ui| {
        ui.set_min_width(220.0);
        let mut query = ui.data(|d| d.get_temp::<String>(query_id)).unwrap_or_default();
        // Read before the field: the text field takes the Enter key event for itself.
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        let field = ui.add(egui::TextEdit::singleline(&mut query).hint_text(hint).desired_width(210.0));
        if button.clicked() {
            query.clear();
            field.request_focus();
        }
        let found = filter(&query, items);
        egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
            if found.is_empty() {
                ui.weak("Nothing matches");
            }
            for (label, value) in &found {
                if ui.selectable_label(label == current, label.as_str()).clicked() {
                    if label != current {
                        picked = Some(value.clone());
                    }
                    ui.close();
                }
            }
        });
        if enter && (field.has_focus() || field.lost_focus()) {
            if let Some((label, value)) = found.first() {
                if label != current {
                    picked = Some(value.clone());
                }
            }
            ui.close();
        }
        ui.data_mut(|d| d.insert_temp(query_id, query));
    });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_word_must_appear_in_any_order() {
        assert!(matches("", "MatFinish Galvanized"));
        assert!(matches("galv", "MatFinish Galvanized"));
        assert!(matches("galvanized matfinish", "MatFinish Galvanized"));
        assert!(matches("  BLUR   slope ", "Blur Slope"));
        assert!(!matches("galv rust", "MatFinish Galvanized"));
        let items = vec![("Blur".to_string(), 0), ("Blur Slope".to_string(), 1), ("Sharpen".to_string(), 2)];
        assert_eq!(filter("blur", &items).iter().map(|(_, v)| *v).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(filter("slope", &items).iter().map(|(_, v)| *v).collect::<Vec<_>>(), vec![1]);
        assert!(filter("xyz", &items).is_empty());
    }
}
