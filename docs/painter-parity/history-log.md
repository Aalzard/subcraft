# Painter parity: History panel and Log window

Sources: https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/history , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/miscellaneous/log , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/edit-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/help-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/main-menu/window-menu , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/interface/settings/shortcuts , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/auto-update , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/features/plugins/plugins , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/installation-and-pre/preferences-and-application-data-location , https://experienceleague.adobe.com/en/docs/substance-3d-painter/using/pipeline-and-integration/configuration/querying-current-software-version
Checked against: origin/main 297dcb3 (2026-11-11)

## History panel

Layout notes: a dockable window holding one plain list, oldest action at the top and newest at the bottom, one row per action with its name. The current state is the highlighted row; rows after it are the redo steps. There are no buttons or options in the panel. The docs describe no setting for the number of steps kept.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| History window (dockable) | Window > Views > History | not documented | partial: History dock tab exists, no Window menu entry to show or hide it | crates/ui/src/app.rs:33 | - |
| List of every action made in the open project | History | one row per action | done | crates/ui/src/panels/history.rs:18 | - |
| Click a row to return to that state | History | - | done | crates/ui/src/panels/history.rs:28 | - |
| Current state row highlighted | History | - | done | crates/ui/src/panels/history.rs:20 | - |
| Rows after the current one stay listed as redo steps | History | - | done | crates/ui/src/panels/history.rs:22 | - |
| New action made from an earlier state discards the later rows | History | always | done | crates/doc/src/history.rs:92 | - |
| One list for the whole project (all Texture Sets) | History | always | done | crates/doc/src/history.rs:23 | - |
| History is not saved in the project; empty after reopening | History | session only | done | crates/project/src/lib.rs:79 | - |
| Undo steps back one row | Edit > Undo | Ctrl+Z | done | crates/app/src/actions.rs:190 | - |
| Redo steps forward one row | Edit > Redo | Ctrl+Y | done | crates/app/src/actions.rs:191 | - |
| Maximum number of history steps | not documented | not documented | partial: fixed at 200 entries, no setting | crates/doc/src/history.rs:35 | - |
| Scroll the list with the wheel or scrollbar | History | - | done | crates/ui/src/panels/history.rs:15 | - |
| Scroll the list with Ctrl+Alt+drag | History (any window) | - | missing | - | - |

## Log window

Layout notes: a dockable window with two tabs, General and Assets. Each tab is a scrolling list of timestamped messages with a severity (information, warning, error). Plugins and scripts can write to it. The same text goes to a log file in the application data folder, reachable from the Help menu.

| Painter item | Where in Painter | Default / range | Status | SubCraft location | PR |
|---|---|---|---|---|---|
| Log window (dockable) | Window > Views > Log | not documented | done: a dock panel, closed until Window > Views > Log or the Docks toolbar opens it | crates/ui/src/panels/log.rs, crates/ui/src/app.rs (Tab::Log) | - |
| General tab | Log | first tab | done: every message the application logs that is not an asset message | crates/ui/src/panels/log.rs, crates/app/src/log_buffer.rs (LogTab) | - |
| Assets tab | Log | second tab | done: messages from the asset library, the Assets panel, libraries and resource import | crates/ui/src/panels/log.rs, crates/app/src/log_buffer.rs (LogTab::for_target) | - |
| Status, progress, warning and error messages from the application | Log > General | - | partial: every logged message is listed (newest 2000); status-bar-only messages that are not logged do not appear | crates/app/src/diagnostics.rs (log_entries) | - |
| Project and scene messages | Log > General | - | partial: status bar and log file only | crates/ui/src/app.rs:1333 | - |
| Export details (written files, problems) | Log > General | - | partial: result shown in the Export window and status bar | crates/ui/src/dialogs.rs:161 | - |
| Baking messages | Log > General (unverified) | - | partial: own log section inside the Bake window | crates/ui/src/dialogs.rs:788 | - |
| Messages written by plugins and scripts | Log > General | - | missing | - | - |
| Assets panel errors and messages | Log > Assets | - | partial: import report window for imported materials only | crates/ui/src/files.rs:114 | - |
| Resource auto-update results (successes and issues) | Log > Assets | - | missing | - | - |
| Per-resource mismatch details (expandable entry) | Log > Assets | - | missing | - | - |
| "Could not be updated in Assets panel" issue entry | Log > Assets | - | missing | - | - |
| "file doesn't exist, can't reload resource" issue entry | Log > Assets | - | missing | - | - |
| Log shortcut button with warning badge in the auto-update menu | Assets > auto-update menu | - | missing | - | - |
| Severity shown per message (info / warning / error) | Log | (unverified) | done: icon and colour per message, warning / error count on each tab, filter (all, warnings and errors, errors only) | crates/ui/src/panels/log.rs | - |
| Clear the log | Log (unverified) | (unverified) | done: Clear hides the messages so far; the log file keeps them | crates/ui/src/panels/log.rs | - |
| Show log (opens the session log file in a text editor) | Help > Show log | - | partial: opens the log folder with the file selected | crates/ui/src/app.rs:1295 | - |
| Export log (save a copy for support) | Help > Export log | - | done: Help > Export log… and the Log's Export… save a copy of this session's log file | crates/ui/src/panels/log.rs (export_log), crates/app/src/log_buffer.rs (export) | - |
| Log file kept in the application data folder | App data (local) | - | done | crates/app/src/diagnostics.rs:238 | - |
| Log attached when reporting a bug | Help > Report a bug | - | partial: bug report links the log path and can copy its tail; user attaches it | crates/ui/src/bug_report.rs:101 | - |
| Scripting can write to the log (alg.log.info and similar) | Scripting API | - | missing | - | - |
