//! Shelf search and category chips (pure logic, unit tested).

/// Active category chip.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Chip {
    #[default]
    All,
    /// Items stored in the user library.
    Mine,
    /// A material category ("Metal", "Plastic", ...).
    Category(String),
}

/// Search text + chip for the visible shelf tab.
#[derive(Clone, Debug, Default)]
pub struct ShelfFilter {
    pub query: String,
    pub chip: Chip,
}

impl ShelfFilter {
    /// True if an item passes the chip and every word of the query appears (case-insensitive)
    /// in its name or category.
    pub fn matches(&self, name: &str, category: Option<&str>, is_user: bool) -> bool {
        let chip_ok = match &self.chip {
            Chip::All => true,
            Chip::Mine => is_user,
            Chip::Category(c) => category.is_some_and(|x| x.eq_ignore_ascii_case(c)),
        };
        if !chip_ok {
            return false;
        }
        let haystack = format!("{} {}", name, category.unwrap_or_default()).to_lowercase();
        self.query.split_whitespace().all(|w| haystack.contains(&w.to_lowercase()))
    }

    pub fn is_active(&self) -> bool {
        !self.query.trim().is_empty() || self.chip != Chip::All
    }
}

/// Distinct categories in first-seen order.
pub fn categories<'a>(items: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in items {
        if !c.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(c)) {
            out.push(c.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(query: &str, chip: Chip) -> ShelfFilter {
        ShelfFilter { query: query.into(), chip }
    }

    #[test]
    fn query_words_match_name_or_category_case_insensitively() {
        assert!(f("", Chip::All).matches("Steel", Some("Metal"), false));
        assert!(f("ste", Chip::All).matches("Steel", Some("Metal"), false));
        assert!(f("METAL", Chip::All).matches("Steel", Some("Metal"), false), "category is searchable");
        assert!(f("glossy red", Chip::All).matches("Plastic Glossy Red", Some("Plastic"), false));
        assert!(!f("glossy blue", Chip::All).matches("Plastic Glossy Red", Some("Plastic"), false), "every word must match");
        assert!(f("  wood  ", Chip::All).matches("Old Wood", None, false), "extra spaces ignored");
    }

    #[test]
    fn chips_filter_by_origin_and_category() {
        assert!(f("", Chip::Mine).matches("My Blue Metal", None, true));
        assert!(!f("", Chip::Mine).matches("Steel", Some("Metal"), false));
        assert!(f("", Chip::Category("metal".into())).matches("Steel", Some("Metal"), false));
        assert!(!f("", Chip::Category("Metal".into())).matches("Rubber Black", Some("Plastic"), false));
        assert!(!f("", Chip::Category("Metal".into())).matches("Round", None, false));
        assert!(!f("", Chip::All).is_active());
        assert!(f("x", Chip::All).is_active());
        assert!(f("", Chip::Mine).is_active());
    }

    #[test]
    fn categories_are_distinct_in_order() {
        assert_eq!(categories(["Metal", "Plastic", "metal", "", "Paint", "Plastic"]), ["Metal", "Plastic", "Paint"]);
    }
}
