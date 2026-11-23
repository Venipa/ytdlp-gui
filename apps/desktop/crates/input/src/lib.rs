use gpui::{KeyBinding, actions};
use ui::{
    Activate, Backspace, BackspaceToStart, BackspaceWord, Copy, Cut, Delete, DeleteToEnd,
    DeleteWord, Deselect, Dismiss, End, FORM_CONTEXT, Home, INPUT_CONTEXT, Left, MENU_CONTEXT,
    NextField, Paste, PreviousField, Right, SelectAll, SelectEnd, SelectHome, SelectLeft,
    SelectNext, SelectPrevious, SelectRight, SelectWordLeft, SelectWordRight, ShowCharacterPalette,
    Space, Submit, WordLeft, WordRight,
};

actions!(
    ytdlpd,
    [
        Quit,
        /// Focus the intake field and paste the clipboard (Ctrl+V on Downloads).
        PasteIntake,
        FocusIntake,
        FocusSearch,
        /// Pause or resume the selected download (Space on a row).
        TogglePause,
        /// Delete the selected entry, keeping the file (Del).
        DeleteEntry,
        /// Delete the selected entry and its file (Shift+Del).
        DeleteFile,
        /// Jump to the first or last row of the list (Home, End).
        FirstRow,
        LastRow,
        OpenSettings,
        ShowDownloads,
        ShowLogs,
        ShowSettings,
        CloseWindow,
        MinimizeWindow,
    ]
);

/// The context of the workspace root.
pub const WORKSPACE_CONTEXT: &str = "Workspace";
/// The context of the focused download list: the keys that act on its selected row.
pub const ROWS_CONTEXT: &str = "Rows";

/// Every key binding, in precedence order: GPUI prefers the deepest matching context and,
/// within one, the binding registered last, so the macOS set at the end overrides the shared one.
pub fn bindings() -> Vec<KeyBinding> {
    let mut bindings = shared();
    if cfg!(target_os = "macos") {
        bindings.extend(macos());
    }
    bindings
}

/// The bindings every platform gets: the keyboard map of docs/design.md, then menus, forms
/// and text editing. Both `cmd-` and `ctrl-` are registered for each shortcut.
fn shared() -> Vec<KeyBinding> {
    let editing = Some(INPUT_CONTEXT);
    let away_from_text = format!("{WORKSPACE_CONTEXT} && !{INPUT_CONTEXT}");
    let form = Some(FORM_CONTEXT);
    let menu = Some(MENU_CONTEXT);
    let browsing =
        format!("{WORKSPACE_CONTEXT} && !{INPUT_CONTEXT} && !{MENU_CONTEXT} && !{FORM_CONTEXT}");
    let rows = format!("{browsing} && {ROWS_CONTEXT}");

    vec![
        KeyBinding::new("down", SelectNext, Some(&browsing)),
        KeyBinding::new("up", SelectPrevious, Some(&browsing)),
        KeyBinding::new("home", FirstRow, Some(&rows)),
        KeyBinding::new("end", LastRow, Some(&rows)),
        KeyBinding::new("enter", Activate, Some(&rows)),
        KeyBinding::new("space", TogglePause, Some(&rows)),
        KeyBinding::new("delete", DeleteEntry, Some(&rows)),
        KeyBinding::new("shift-delete", DeleteFile, Some(&rows)),
        KeyBinding::new("ctrl-v", PasteIntake, Some(&away_from_text)),
        KeyBinding::new("cmd-v", PasteIntake, Some(&away_from_text)),
        KeyBinding::new("ctrl-l", FocusIntake, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("cmd-l", FocusIntake, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("/", FocusSearch, Some(&away_from_text)),
        KeyBinding::new("ctrl-f", FocusSearch, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("cmd-f", FocusSearch, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("ctrl-,", OpenSettings, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("ctrl-1", ShowDownloads, None),
        KeyBinding::new("ctrl-2", ShowLogs, None),
        KeyBinding::new("ctrl-3", ShowSettings, None),
        KeyBinding::new("cmd-1", ShowDownloads, None),
        KeyBinding::new("cmd-2", ShowLogs, None),
        KeyBinding::new("cmd-3", ShowSettings, None),
        KeyBinding::new("escape", Deselect, Some(&browsing)),
        KeyBinding::new("escape", Dismiss, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("down", SelectNext, menu),
        KeyBinding::new("up", SelectPrevious, menu),
        KeyBinding::new("tab", SelectNext, menu),
        KeyBinding::new("shift-tab", SelectPrevious, menu),
        KeyBinding::new("enter", Submit, menu),
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("ctrl-q", Quit, None),
        KeyBinding::new("backspace", Backspace, editing),
        KeyBinding::new("ctrl-backspace", BackspaceWord, editing),
        KeyBinding::new("delete", Delete, editing),
        KeyBinding::new("ctrl-delete", DeleteWord, editing),
        KeyBinding::new("left", Left, editing),
        KeyBinding::new("right", Right, editing),
        KeyBinding::new("ctrl-left", WordLeft, editing),
        KeyBinding::new("ctrl-right", WordRight, editing),
        KeyBinding::new("shift-left", SelectLeft, editing),
        KeyBinding::new("shift-right", SelectRight, editing),
        KeyBinding::new("shift-ctrl-left", SelectWordLeft, editing),
        KeyBinding::new("shift-ctrl-right", SelectWordRight, editing),
        KeyBinding::new("home", Home, editing),
        KeyBinding::new("end", End, editing),
        KeyBinding::new("cmd-left", Home, editing),
        KeyBinding::new("cmd-right", End, editing),
        KeyBinding::new("shift-home", SelectHome, editing),
        KeyBinding::new("shift-end", SelectEnd, editing),
        KeyBinding::new("shift-cmd-left", SelectHome, editing),
        KeyBinding::new("shift-cmd-right", SelectEnd, editing),
        KeyBinding::new("cmd-a", SelectAll, editing),
        KeyBinding::new("ctrl-a", SelectAll, editing),
        KeyBinding::new("cmd-v", Paste, editing),
        KeyBinding::new("ctrl-v", Paste, editing),
        KeyBinding::new("cmd-c", Copy, editing),
        KeyBinding::new("ctrl-c", Copy, editing),
        KeyBinding::new("cmd-x", Cut, editing),
        KeyBinding::new("ctrl-x", Cut, editing),
        KeyBinding::new("ctrl-cmd-space", ShowCharacterPalette, editing),
        KeyBinding::new("space", Space, editing),
        KeyBinding::new("escape", Dismiss, editing),
        KeyBinding::new("enter", Submit, form),
        KeyBinding::new("escape", Dismiss, form),
        KeyBinding::new("tab", NextField, form),
        KeyBinding::new("shift-tab", PreviousField, form),
        // everywhere else Tab walks the tab stops: nav, chips, rows, buttons
        KeyBinding::new("tab", NextField, Some(WORKSPACE_CONTEXT)),
        KeyBinding::new("shift-tab", PreviousField, Some(WORKSPACE_CONTEXT)),
    ]
}

/// What a Mac user expects on top of the shared set: the standard window and application
/// shortcuts, and the Cocoa text-field conventions, `alt` for words, `cmd` for the line and the
/// Emacs control keys.
fn macos() -> Vec<KeyBinding> {
    let editing = Some(INPUT_CONTEXT);

    vec![
        KeyBinding::new("alt-left", WordLeft, editing),
        KeyBinding::new("alt-right", WordRight, editing),
        KeyBinding::new("shift-alt-left", SelectWordLeft, editing),
        KeyBinding::new("shift-alt-right", SelectWordRight, editing),
        KeyBinding::new("alt-backspace", BackspaceWord, editing),
        KeyBinding::new("alt-delete", DeleteWord, editing),
        KeyBinding::new("cmd-backspace", BackspaceToStart, editing),
        KeyBinding::new("cmd-delete", DeleteToEnd, editing),
        KeyBinding::new("cmd-up", Home, editing),
        KeyBinding::new("cmd-down", End, editing),
        KeyBinding::new("shift-cmd-up", SelectHome, editing),
        KeyBinding::new("shift-cmd-down", SelectEnd, editing),
        KeyBinding::new("ctrl-a", Home, editing),
        KeyBinding::new("ctrl-e", End, editing),
        KeyBinding::new("ctrl-b", Left, editing),
        KeyBinding::new("ctrl-f", Right, editing),
        KeyBinding::new("ctrl-h", Backspace, editing),
        KeyBinding::new("ctrl-d", Delete, editing),
        KeyBinding::new("ctrl-k", DeleteToEnd, editing),
    ]
}
