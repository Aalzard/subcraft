//! The messages behind Painter's Log window: every log record the application writes, kept in
//! memory with its severity and the Log tab it belongs to (General, or Assets for the Assets
//! panel, libraries and resource import), plus the filters and the export the window uses.
//! Pure and unit-tested; [`crate::diagnostics`] feeds the live buffer.

use std::collections::VecDeque;
use std::path::Path;

/// Messages the Log keeps (older ones drop off; the log file keeps everything).
pub const LOG_ENTRIES: usize = 2000;

/// Severity of a message, as the Log shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    pub fn from_record(level: log::Level) -> Self {
        match level {
            log::Level::Error => LogLevel::Error,
            log::Level::Warn => LogLevel::Warning,
            _ => LogLevel::Info,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LogLevel::Info => "Info",
            LogLevel::Warning => "Warning",
            LogLevel::Error => "Error",
        }
    }
}

/// Painter's two Log tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LogTab {
    #[default]
    General,
    Assets,
}

impl LogTab {
    pub const ALL: [LogTab; 2] = [LogTab::General, LogTab::Assets];

    /// The tab of a record written from `target` (its module path): the asset library crate, the
    /// Assets panel, libraries and resource import go to Assets, everything else to General.
    pub fn for_target(target: &str) -> Self {
        const ASSETS: [&str; 6] = ["subcraft_assets", "::shelf", "::libraries", "::import_resources", "::asset_preview", "::library"];
        if ASSETS.iter().any(|m| if m.starts_with("::") { target.contains(m) } else { target.starts_with(m) }) {
            LogTab::Assets
        } else {
            LogTab::General
        }
    }
}

/// One message of the Log.
#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    /// Increases by one per message, never reused (the Clear button hides up to a number).
    pub seq: u64,
    /// "2026-11-14 10:03:07" (UTC, as in the log file).
    pub time: String,
    pub level: LogLevel,
    pub tab: LogTab,
    pub text: String,
}

/// The newest [`LOG_ENTRIES`] messages.
#[derive(Clone, Debug, Default)]
pub struct LogBuffer {
    entries: VecDeque<LogEntry>,
    next_seq: u64,
}

impl LogBuffer {
    pub fn push(&mut self, time: String, level: LogLevel, target: &str, text: String) {
        if self.entries.len() >= LOG_ENTRIES {
            self.entries.pop_front();
        }
        self.next_seq += 1;
        self.entries.push_back(LogEntry { seq: self.next_seq, time, level, tab: LogTab::for_target(target), text });
    }

    pub fn entries(&self) -> impl Iterator<Item = &LogEntry> {
        self.entries.iter()
    }

    /// Sequence number of the newest message (0 before any).
    pub fn last_seq(&self) -> u64 {
        self.next_seq
    }
}

/// What the Log shows: messages of `tab` at `min` or above, written after the Clear at `cleared`.
pub fn visible<'a>(entries: &'a [LogEntry], tab: LogTab, min: LogLevel, cleared: u64) -> impl Iterator<Item = &'a LogEntry> {
    entries.iter().filter(move |e| e.tab == tab && e.level >= min && e.seq > cleared)
}

/// Warnings and errors in `tab` since the Clear at `cleared` (the tab's badge).
pub fn problems(entries: &[LogEntry], tab: LogTab, cleared: u64) -> (usize, usize) {
    entries.iter().filter(|e| e.tab == tab && e.seq > cleared).fold((0, 0), |(w, er), e| match e.level {
        LogLevel::Warning => (w + 1, er),
        LogLevel::Error => (w, er + 1),
        LogLevel::Info => (w, er),
    })
}

/// Help > Export log: a copy of the session's log file at `dest`, or, without a log file, the
/// messages still in memory.
pub fn export(dest: &Path, log_file: Option<&Path>, in_memory: &[String]) -> std::io::Result<()> {
    match log_file.filter(|f| f.is_file()) {
        Some(f) => std::fs::copy(f, dest).map(|_| ()),
        None => {
            let mut text = in_memory.join("\n");
            text.push('\n');
            std::fs::write(dest, text)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(seq: u64, level: LogLevel, tab: LogTab) -> LogEntry {
        LogEntry { seq, time: String::new(), level, tab, text: format!("m{seq}") }
    }

    #[test]
    fn records_land_in_the_assets_tab_only_from_asset_code() {
        for t in ["subcraft_assets::presets", "subcraft_ui::panels::shelf", "subcraft_ui::libraries", "subcraft_ui::import_resources", "subcraft_app::library"] {
            assert_eq!(LogTab::for_target(t), LogTab::Assets, "{t}");
        }
        for t in ["subcraft_app::session", "subcraft_ui::viewport", "wgpu_core::device", "subcraft_export", "subcraft_app::diagnostics"] {
            assert_eq!(LogTab::for_target(t), LogTab::General, "{t}");
        }
    }

    #[test]
    fn the_buffer_numbers_messages_and_keeps_the_newest() {
        let mut b = LogBuffer::default();
        assert_eq!(b.last_seq(), 0);
        b.push("t".into(), LogLevel::from_record(log::Level::Warn), "subcraft_ui::panels::shelf", "thumb failed".into());
        b.push("t".into(), LogLevel::from_record(log::Level::Debug), "subcraft_app::session", "saved".into());
        let all: Vec<&LogEntry> = b.entries().collect();
        assert_eq!((all[0].seq, all[0].level, all[0].tab), (1, LogLevel::Warning, LogTab::Assets));
        assert_eq!((all[1].seq, all[1].level, all[1].tab), (2, LogLevel::Info, LogTab::General));
        for i in 0..LOG_ENTRIES + 5 {
            b.push("t".into(), LogLevel::Info, "x", format!("{i}"));
        }
        assert_eq!(b.entries().count(), LOG_ENTRIES);
        assert_eq!(b.last_seq(), LOG_ENTRIES as u64 + 7);
        assert_eq!(b.entries().next().unwrap().seq, 8, "the oldest dropped off");
        assert!(b.entries().zip(b.entries().skip(1)).all(|(a, c)| c.seq == a.seq + 1));
    }

    #[test]
    fn filters_follow_the_tab_the_severity_and_the_clear() {
        let e = vec![
            entry(1, LogLevel::Info, LogTab::General),
            entry(2, LogLevel::Warning, LogTab::General),
            entry(3, LogLevel::Error, LogTab::Assets),
            entry(4, LogLevel::Error, LogTab::General),
            entry(5, LogLevel::Warning, LogTab::Assets),
        ];
        let seqs = |tab, min, cleared| visible(&e, tab, min, cleared).map(|x| x.seq).collect::<Vec<_>>();
        assert_eq!(seqs(LogTab::General, LogLevel::Info, 0), [1, 2, 4]);
        assert_eq!(seqs(LogTab::General, LogLevel::Warning, 0), [2, 4]);
        assert_eq!(seqs(LogTab::General, LogLevel::Error, 0), [4]);
        assert_eq!(seqs(LogTab::Assets, LogLevel::Info, 0), [3, 5]);
        assert_eq!(seqs(LogTab::General, LogLevel::Info, 2), [4], "Clear hides what came before it");
        assert_eq!(problems(&e, LogTab::General, 0), (1, 1));
        assert_eq!(problems(&e, LogTab::Assets, 0), (1, 1));
        assert_eq!(problems(&e, LogTab::Assets, 4), (1, 0));
        assert_eq!(problems(&e, LogTab::General, 5), (0, 0));
    }

    #[test]
    fn export_copies_the_log_file_or_writes_the_messages_in_memory() {
        let dir = std::env::temp_dir().join(format!("subcraft-log-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("subcraft.log");
        std::fs::write(&file, "line one\nline two\n").unwrap();
        let out = dir.join("copy.log");
        export(&out, Some(&file), &["ignored".into()]).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "line one\nline two\n");
        let out2 = dir.join("memory.log");
        export(&out2, Some(&dir.join("missing.log")), &["a".into(), "b".into()]).unwrap();
        assert_eq!(std::fs::read_to_string(&out2).unwrap(), "a\nb\n");
        export(&out2, None, &[]).unwrap();
        assert_eq!(std::fs::read_to_string(&out2).unwrap(), "\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
