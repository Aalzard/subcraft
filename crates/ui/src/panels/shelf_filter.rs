//! Shelf search: Painter's query language, category chips and the path of an asset (pure logic,
//! unit tested).
//!
//! A query is a list of terms that must all match. Terms are separated by spaces or commas.
//! * `n:` name, `s:` library (shelf), `p:` path, `u:` usage; without a prefix a term searches the
//!   name, the path and the tags. After a prefix, commas make an "or" group: `n:rust,dirt`.
//! * `"quoted text"` and `a\ b` keep spaces (and commas) inside one pattern.
//! * A leading `-` excludes: `u:texture n:-normal`.
//! * The last character picks how a pattern matches: none is approximate (a typo is forgiven),
//!   `!` contains, `#` regular expression, `=` equal, `^` starts with, `&` ends with.
//! * A usage keyword typed alone (`textures`, `alphas`, `images`...) filters by usage.

/// Active category chip.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Chip {
    #[default]
    All,
    /// Items stored in the user library.
    Mine,
    Tileable,
    /// A material category ("Metal", "Plastic", ...).
    Category(String),
}

/// Library of the built-in content (`s:starter_assets`).
pub const STARTER: &str = "starter_assets";
/// Library of the user's own files, Documents/SubCraft/Library (`s:your_assets`).
pub const YOURS: &str = "your_assets";

/// Library ids with the names the breadcrumbs show.
pub const LIBRARIES: [(&str, &str); 3] = [(STARTER, "Starter assets"), (YOURS, "Your assets"), ("session", "Current session")];

/// Display name of a library id.
pub fn library_name(id: &str) -> &str {
    LIBRARIES.iter().find(|l| l.0 == id).map_or(id, |l| l.1)
}

/// Usage keywords (`u:` values) and the other words that mean the same usage.
const USAGES: [(&str, &[&str]); 9] = [
    ("basematerial", &["basematerials", "materials"]),
    ("smartmaterial", &["smartmaterials"]),
    ("smartmask", &["smartmasks"]),
    ("filter", &["filters"]),
    ("brush", &["brushes"]),
    ("alpha", &["alphas"]),
    ("texture", &["textures", "image", "images"]),
    ("normalmap", &["normalmaps"]),
    ("environment", &["environments"]),
];

/// The usage keyword `word` names (`"Textures"` -> `"texture"`), if any.
pub fn canonical_usage(word: &str) -> Option<&'static str> {
    let w = word.to_lowercase();
    USAGES.iter().find(|(u, aliases)| *u == w || aliases.contains(&w.as_str())).map(|u| u.0)
}

/// One asset as the search sees it.
#[derive(Clone, Debug)]
pub struct Entry<'a> {
    pub name: &'a str,
    /// Usage keyword (`"basematerial"`, `"alpha"`...).
    pub usage: &'a str,
    /// Library id ([`STARTER`], [`YOURS`]).
    pub library: &'a str,
    /// Folder inside the library (the asset's category), if any.
    pub folder: Option<&'a str>,
    /// Extra searchable words.
    pub tags: &'a [String],
    pub tileable: bool,
}

impl Entry<'_> {
    /// `/library/folder/` (or `/library/`), the form `p:` and Filter by path use.
    pub fn path(&self) -> String {
        match self.folder {
            Some(f) if !f.is_empty() => format!("/{}/{}/", self.library, f),
            _ => format!("/{}/", self.library),
        }
    }
}

/// Path of a folder of a library, as [`Entry::path`] writes it.
pub fn folder_path(library: &str, folder: Option<&str>) -> String {
    match folder {
        Some(f) => format!("/{library}/{f}/"),
        None => format!("/{library}/"),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Any,
    Name,
    Library,
    Path,
    Usage,
}

/// How one pattern compares (its suffix character).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// No suffix: contains, or within a typo of a word.
    Approximate,
    /// `!`
    Contains,
    /// `#`
    Regex,
    /// `=`
    Equal,
    /// `^`
    Starts,
    /// `&`
    Ends,
}

#[derive(Clone, Debug)]
pub struct Pattern {
    /// Lowercase text without the suffix.
    pub text: String,
    pub mode: Mode,
    regex: Option<regex_lite::Regex>,
}

impl Pattern {
    fn new(text: String, mode: Mode) -> Self {
        let regex = (mode == Mode::Regex).then(|| regex_lite::RegexBuilder::new(&text).case_insensitive(true).build().ok()).flatten();
        Self { text: text.to_lowercase(), mode, regex }
    }

    fn matches(&self, value: &str) -> bool {
        let v = value.to_lowercase();
        match self.mode {
            Mode::Contains => v.contains(&self.text),
            Mode::Equal => v == self.text,
            Mode::Starts => v.starts_with(&self.text),
            Mode::Ends => v.ends_with(&self.text),
            // An invalid expression matches nothing (the user is still typing it).
            Mode::Regex => self.regex.as_ref().is_some_and(|r| r.is_match(value)),
            Mode::Approximate => v.contains(&self.text) || approximately_in(&self.text, &v),
        }
    }
}

/// True when `pattern` is within one typo (two for long patterns) of a word of `value`, or of
/// the start of a word (so a misspelt word being typed still finds its asset).
fn approximately_in(pattern: &str, value: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let allowed = match p.len() {
        0..=3 => return false,
        4..=6 => 1,
        _ => 2,
    };
    value.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).any(|w| {
        let w: Vec<char> = w.chars().collect();
        let start = &w[..w.len().min(p.len())];
        distance(&p, &w) <= allowed || (w.len() > p.len() && distance(&p, start) <= allowed)
    })
}

/// Edit distance with adjacent swaps (optimal string alignment).
fn distance(a: &[char], b: &[char]) -> usize {
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        d[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// A term: one field, an "or" group of patterns, possibly excluded.
#[derive(Clone, Debug)]
pub struct Term {
    pub field: Field,
    pub negate: bool,
    pub patterns: Vec<Pattern>,
}

impl Term {
    fn matches(&self, e: &Entry) -> bool {
        let path = e.path();
        let hit = self.patterns.iter().any(|p| match self.field {
            Field::Name => p.matches(e.name),
            Field::Path => p.matches(&path),
            Field::Library => p.matches(e.library) || p.matches(library_name(e.library)),
            Field::Usage => {
                // `u:textures` and `u:texture` are the same usage. A regular expression is used as
                // typed (QA 2026-11: `u:texture#` was rebuilt without its compiled expression and
                // matched nothing).
                let canon = (p.mode != Mode::Regex).then(|| canonical_usage(&p.text)).flatten().map(|u| Pattern { text: u.into(), mode: if p.mode == Mode::Approximate { Mode::Equal } else { p.mode }, regex: None });
                canon.as_ref().unwrap_or(p).matches(e.usage)
            }
            Field::Any => p.matches(e.name) || p.matches(&path) || e.tags.iter().any(|t| p.matches(t)),
        });
        hit != self.negate
    }
}

/// A character of the query and whether it was quoted or escaped (then it never separates
/// terms and is never a suffix).
type Lit = (char, bool);

/// Whitespace-separated pieces, quotes and escapes resolved, each with its raw text.
fn tokens(query: &str) -> Vec<(Vec<Lit>, String)> {
    let mut out = Vec::new();
    let mut cur: Vec<Lit> = Vec::new();
    let mut raw = String::new();
    let mut quoted = false;
    let mut chars = query.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                raw.push(c);
                if let Some(n) = chars.next() {
                    raw.push(n);
                    cur.push((n, true));
                }
            }
            '"' => {
                raw.push(c);
                quoted = !quoted;
            }
            c if c.is_whitespace() && !quoted => {
                if !raw.is_empty() {
                    out.push((std::mem::take(&mut cur), std::mem::take(&mut raw)));
                }
            }
            c => {
                raw.push(c);
                cur.push((c, quoted));
            }
        }
    }
    if !raw.is_empty() {
        out.push((cur, raw));
    }
    out
}

/// Split on unquoted commas.
fn split_commas(lits: &[Lit]) -> Vec<&[Lit]> {
    lits.split(|&(c, literal)| c == ',' && !literal).filter(|s| !s.is_empty()).collect()
}

/// A pattern from its characters: the last unquoted character may be a mode suffix.
fn pattern(lits: &[Lit]) -> Option<Pattern> {
    let (mode, body) = match lits.last() {
        Some(&(c, false)) => match c {
            '!' => (Mode::Contains, &lits[..lits.len() - 1]),
            '#' => (Mode::Regex, &lits[..lits.len() - 1]),
            '=' => (Mode::Equal, &lits[..lits.len() - 1]),
            '^' => (Mode::Starts, &lits[..lits.len() - 1]),
            '&' => (Mode::Ends, &lits[..lits.len() - 1]),
            _ => (Mode::Approximate, lits),
        },
        _ => (Mode::Approximate, lits),
    };
    let text: String = body.iter().map(|l| l.0).collect();
    (!text.is_empty()).then(|| Pattern::new(text, mode))
}

/// Field prefix (`n:`...) of a token, and the rest.
fn prefix(lits: &[Lit]) -> (Field, &[Lit]) {
    if lits.len() >= 2 && lits[1] == (':', false) && !lits[0].1 {
        let field = match lits[0].0.to_ascii_lowercase() {
            'n' => Some(Field::Name),
            's' => Some(Field::Library),
            'p' => Some(Field::Path),
            'u' => Some(Field::Usage),
            _ => None,
        };
        if let Some(f) = field {
            return (f, &lits[2..]);
        }
    }
    (Field::Any, lits)
}

/// Leading unquoted `-`: exclusion.
fn negation(lits: &[Lit]) -> (bool, &[Lit]) {
    match lits.first() {
        Some(&('-', false)) if lits.len() > 1 => (true, &lits[1..]),
        _ => (false, lits),
    }
}

/// The terms of one whitespace-separated token.
fn token_terms(lits: &[Lit]) -> Vec<Term> {
    let (field, rest) = prefix(lits);
    if field != Field::Any {
        let (negate, rest) = negation(rest);
        let patterns: Vec<Pattern> = split_commas(rest).into_iter().filter_map(pattern).collect();
        return if patterns.is_empty() { Vec::new() } else { vec![Term { field, negate, patterns }] };
    }
    // Without a prefix a comma separates terms that must all match ("cobalt, gold").
    split_commas(rest)
        .into_iter()
        .filter_map(|piece| {
            let (negate, piece) = negation(piece);
            let p = pattern(piece)?;
            // A usage keyword typed on its own filters by usage.
            let all_plain = piece.iter().all(|l| !l.1);
            match canonical_usage(&p.text).filter(|_| p.mode == Mode::Approximate && all_plain) {
                Some(u) => Some(Term { field: Field::Usage, negate, patterns: vec![Pattern::new(u.into(), Mode::Equal)] }),
                None => Some(Term { field: Field::Any, negate, patterns: vec![p] }),
            }
        })
        .collect()
}

/// A parsed query: every term must match.
#[derive(Clone, Debug, Default)]
pub struct Query {
    pub terms: Vec<Term>,
}

impl Query {
    pub fn parse(text: &str) -> Self {
        Self { terms: tokens(text).iter().flat_map(|(lits, _)| token_terms(lits)).collect() }
    }

    pub fn matches(&self, e: &Entry) -> bool {
        self.terms.iter().all(|t| t.matches(e))
    }
}

/// A saved query split into the parts the Shelf shows: the asset types (`u:x=,y=`), the
/// selected folders (`p:"/lib/folder/"^` or `s:lib=`) and the text left for the search field.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QueryParts {
    pub usages: Vec<String>,
    pub paths: Vec<String>,
    pub text: String,
}

/// Inverse of [`compose`]: tokens that only list exact usages or path starts become types and
/// folders; everything else stays text.
pub fn decompose(query: &str) -> QueryParts {
    let mut parts = QueryParts::default();
    let mut text = Vec::new();
    for (lits, raw) in tokens(query) {
        let (field, rest) = prefix(&lits);
        let (negate, rest) = negation(rest);
        let patterns: Vec<Pattern> = split_commas(rest).into_iter().filter_map(pattern).collect();
        let usages: Option<Vec<String>> = (field == Field::Usage && !negate && !patterns.is_empty())
            .then(|| patterns.iter().map(|p| (p.mode == Mode::Equal).then(|| canonical_usage(&p.text)).flatten().map(String::from)).collect())
            .flatten();
        let paths: Option<Vec<String>> = (!negate && !patterns.is_empty())
            .then(|| match field {
                Field::Path => patterns.iter().map(|p| (p.mode == Mode::Starts && p.text.starts_with('/') && p.text.ends_with('/')).then(|| p.text.clone())).collect(),
                Field::Library => patterns.iter().map(|p| (p.mode == Mode::Equal).then(|| folder_path(&p.text, None))).collect(),
                _ => None,
            })
            .flatten();
        if let Some(u) = usages {
            parts.usages.extend(u);
        } else if let Some(p) = paths {
            parts.paths.extend(p);
        } else {
            text.push(raw);
        }
    }
    parts.text = text.join(" ");
    parts
}

/// Quote a pattern for a query when it holds characters the parser would read.
fn quote(text: &str) -> String {
    if text.chars().any(|c| c.is_whitespace() || matches!(c, ',' | '"' | '\\' | '!' | '#' | '=' | '^' | '&' | '-' | ':')) {
        format!("\"{}\"", text.replace('"', ""))
    } else {
        text.to_string()
    }
}

/// One query string for the text, the asset types and the folders (Painter's saved-search form).
pub fn compose(text: &str, usages: &[&str], paths: &[String]) -> String {
    let mut out = Vec::new();
    if !usages.is_empty() {
        out.push(format!("u:{}", usages.iter().map(|u| format!("{u}=")).collect::<Vec<_>>().join(",")));
    }
    // A whole library is `s:id=`; a folder is the start of a path.
    let (libraries, folders): (Vec<&String>, Vec<&String>) = paths.iter().partition(|p| p.matches('/').count() == 2);
    if !libraries.is_empty() {
        out.push(format!("s:{}", libraries.iter().map(|p| format!("{}=", quote(p.trim_matches('/')))).collect::<Vec<_>>().join(",")));
    }
    if !folders.is_empty() {
        out.push(format!("p:{}", folders.iter().map(|p| format!("{}^", quote(p))).collect::<Vec<_>>().join(",")));
    }
    if !text.trim().is_empty() {
        out.push(text.trim().to_string());
    }
    out.join(" ")
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
        self.matches_tagged(name, category, is_user, false)
    }

    pub fn matches_tagged(&self, name: &str, category: Option<&str>, is_user: bool, tileable: bool) -> bool {
        let tags: Vec<String> = category.into_iter().map(String::from).collect();
        self.matches_entry(&Entry { name, usage: "", library: if is_user { YOURS } else { STARTER }, folder: category, tags: &tags, tileable }, &Query::parse(&self.query))
    }

    /// The chip and a parsed query (parse once per frame, not per asset).
    pub fn matches_entry(&self, e: &Entry, query: &Query) -> bool {
        let chip_ok = match &self.chip {
            Chip::All => true,
            Chip::Mine => e.library == YOURS,
            Chip::Tileable => e.tileable,
            Chip::Category(c) => e.folder.is_some_and(|x| x.eq_ignore_ascii_case(c)) || e.tags.iter().any(|t| t.eq_ignore_ascii_case(c)),
        };
        chip_ok && query.matches(e)
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

    fn entry<'a>(name: &'a str, usage: &'a str, library: &'a str, folder: Option<&'a str>) -> Entry<'a> {
        Entry { name, usage, library, folder, tags: &[], tileable: false }
    }

    fn q(query: &str, e: &Entry) -> bool {
        Query::parse(query).matches(e)
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

    #[test]
    fn field_prefixes_search_one_attribute() {
        let steel = entry("Steel Brushed", "basematerial", STARTER, Some("Metal"));
        let mine = entry("Rust Drips", "texture", YOURS, Some("Grunges"));
        assert!(q("n:steel", &steel) && !q("n:metal", &steel), "n: is the name only");
        assert!(q("metal", &steel), "no prefix also searches the path");
        assert!(q("p:metal", &steel) && !q("p:steel", &steel));
        assert!(q("s:starter_assets", &steel) && !q("s:your_assets", &steel));
        assert!(q("s:yours", &mine), "the library's display name counts");
        assert!(q("u:basematerial", &steel) && q("u:materials", &steel), "plural usage words are the same usage");
        assert!(!q("u:texture", &steel) && q("u:images", &mine));
        assert!(q("u:texture#", &mine) && q("u:^tex#", &mine) && !q("u:texture#", &steel), "a usage regex is applied as typed");
        assert!(q("N:STEEL", &steel), "prefixes ignore case");
    }

    #[test]
    fn or_groups_commas_and_exclusion() {
        let rust = entry("Rust Drips", "texture", YOURS, None);
        let dirt = entry("Dirt", "texture", STARTER, None);
        let normal = entry("Rivets Normal", "texture", STARTER, None);
        for e in [&rust, &dirt] {
            assert!(q("n:rust,dirt", e), "after a prefix, commas are alternatives");
        }
        assert!(!q("rust, dirt", &dirt), "between plain terms a comma means both");
        assert!(q("rust, drips", &rust));
        assert!(q("u:texture n:-normal", &dirt) && !q("u:texture n:-normal", &normal));
        assert!(!q("-dirt", &dirt) && q("-dirt", &rust), "a bare - excludes too");
    }

    #[test]
    fn suffixes_pick_the_matching_function() {
        let dirt = entry("Dirt", "texture", STARTER, None);
        let drips = entry("Drips", "texture", STARTER, None);
        assert!(q("di!", &dirt) && !q("di!", &drips), "! is contains");
        assert!(q("dirt=", &dirt) && !q("dir=", &dirt), "= is equal");
        assert!(q("dr^", &drips) && !q("ri^", &drips), "^ starts with");
        assert!(q("ips&", &drips) && !q("dri&", &drips), "& ends with");
        assert!(q("^d.r[tp]#", &dirt) && !q("^d.r[tp]#", &drips), "# is a regular expression");
        assert!(!q("([#", &dirt), "an unfinished expression matches nothing instead of panicking");
    }

    #[test]
    fn approximate_matching_forgives_a_typo_unless_disabled() {
        let e = entry("Concrete Rough", "basematerial", STARTER, None);
        assert!(q("concreet", &e), "a swapped pair is one typo");
        assert!(q("conrete", &e), "a missing letter");
        assert!(q("roguh", &e));
        assert!(!q("concreet!", &e), "! turns typo tolerance off");
        assert!(!q("cat", &e), "short words need an exact substring");
        assert!(!q("plastik", &e));
        assert!(q("concr", &entry("Concrete", "basematerial", STARTER, None)), "a word being typed matches its start");
    }

    #[test]
    fn quotes_and_backslashes_keep_spaces_in_one_pattern() {
        let e = entry("Paint Peeling Old", "smartmaterial", STARTER, None);
        assert!(q("\"paint peeling\"!", &e) && q("paint\\ peeling!", &e));
        assert!(!q("\"peeling paint\"!", &e), "a quoted phrase is one pattern, not two words");
        assert!(q("n:\"paint peeling\",xyz", &e));
        let comma = entry("A, B", "texture", STARTER, None);
        assert!(q("\"a, b\"!", &comma), "a quoted comma is text");
    }

    #[test]
    fn a_usage_word_on_its_own_filters_by_usage() {
        let tex = entry("Ambient Grunge", "texture", STARTER, None);
        let mat = entry("Ambient Steel", "basematerial", STARTER, None);
        assert!(q("images ambient", &tex) && !q("images ambient", &mat));
        assert!(q("textures", &tex) && !q("textures", &mat));
        assert!(q("\"textures\"", &entry("textures pack", "alpha", STARTER, None)), "a quoted keyword is plain text");
    }

    #[test]
    fn saved_query_round_trips_types_folders_and_text() {
        let paths = vec![folder_path(STARTER, Some("Metal")), folder_path(YOURS, None), folder_path(STARTER, Some("Old Wood"))];
        let text = compose("rust n:-normal", &["basematerial", "smartmaterial"], &paths);
        assert_eq!(text, "u:basematerial=,smartmaterial= s:your_assets= p:/starter_assets/Metal/^,\"/starter_assets/Old Wood/\"^ rust n:-normal");
        let parts = decompose(&text);
        assert_eq!(parts.usages, ["basematerial", "smartmaterial"]);
        assert_eq!(parts.paths, ["/your_assets/", "/starter_assets/metal/", "/starter_assets/old wood/"]);
        assert_eq!(parts.text, "rust n:-normal");
        // Painter's own Shelf.ini examples.
        assert_eq!(decompose("u:procedural=").text, "u:procedural=", "an unknown usage stays text");
        let p = decompose("p:/allegorithmic/^ u:filters=");
        assert_eq!((p.usages, p.paths, p.text), (vec!["filter".to_string()], vec!["/allegorithmic/".to_string()], String::new()));
        // The composed query selects what the parts select.
        let wood = Entry { name: "Rust Plank", usage: "basematerial", library: STARTER, folder: Some("Old Wood"), tags: &[], tileable: false };
        assert!(Query::parse(&text.replace("s:your_assets= ", "")).matches(&wood));
    }
}
