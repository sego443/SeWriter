use eframe::egui;
use chrono::Local;
use global_hotkey::{GlobalHotKeyManager, hotkey::{Code, HotKey, Modifiers}, GlobalHotKeyEvent};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static HOTKEY_FIRED: AtomicBool = AtomicBool::new(false);

fn default_font_size() -> f32 { 16.0 }

// Title input uses the Heading font (24 px); same padding formula as the editor.
const TITLE_FONT_SIZE: f32 = 24.0;
const TITLE_CURSOR_HEIGHT: f32 = TITLE_FONT_SIZE + 4.0;
const TITLE_LINE_HEIGHT: f32 = TITLE_CURSOR_HEIGHT + 8.0;

// Command palette entries, sorted alphabetically by name.
const COMMANDS: &[(&str, &str)] = &[
    ("/config", "adjust settings"),
    ("/finish", "save and close, start fresh next time"),
    ("/new",    "save current and start a new file"),
    ("/re",     "open a previous file from vault"),
    ("/title",  "rename the current title"),
    ("/vault",  "manage vault"),
];

// Vault sub-commands: (name, description).
const VAULT_SUBCMDS: &[(&str, &str)] = &[
    ("new",   "switch to a new vault"),
    ("reset", "move vault to a new location"),
];

#[derive(Serialize, Deserialize, Default)]
struct AppState {
    vault_path: Option<PathBuf>,
    current_title: String,
    current_content: String,
    last_edit_date: Option<String>,
    window_size: Option<[f32; 2]>,
    #[serde(default = "default_font_size")]
    font_size: f32,
}

struct SeWriterApp {
    state: AppState,
    config_path: PathBuf,
    input_mode: InputMode,
    show_save_dialog: bool,
    request_focus: bool,
    last_sent_title: String,
    is_hidden: bool,
    ime_preedit: String,
    command_input: String,
    command_selected: usize,
    command_panel_id: u32,      // changes each open so egui forgets stored cursor state
    command_parent: Option<String>, // tracks current command sub-level
    rename_old_title: String,       // saved title for Esc-cancel in RenameTitle mode
    command_re_selected_title: String, // base title chosen in /re level 2
    command_saved_selections: std::collections::HashMap<String, usize>, // saved selection per level
    command_panel_needs_scroll: bool, // scroll to selected on next frame
    cursor_visible: bool,             // current blink state (true = shown)
    cursor_blink_start: f64,          // time of last toggle; drives next repaint schedule
}

#[derive(PartialEq)]
enum InputMode {
    SelectVault,
    InputTitle,
    RenameTitle,
    EditContent,
    CommandPanel,
}

impl SeWriterApp {
    fn new() -> Self {
        let config_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("sewriter")
            .join("state.json");

        let mut state: AppState = if config_path.exists() {
            fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            AppState::default()
        };

        let input_mode = if state.vault_path.is_none() {
            InputMode::SelectVault
        } else {
            let today = Local::now().format("%Y-%m-%d").to_string();
            if state.last_edit_date.as_ref() == Some(&today) && !state.current_title.is_empty() {
                InputMode::EditContent
            } else {
                InputMode::InputTitle
            }
        };

        // New day: clear stale content so editor opens blank
        if input_mode == InputMode::InputTitle {
            state.current_content = String::new();
        }

        Self {
            state,
            config_path,
            input_mode,
            show_save_dialog: false,
            request_focus: true,
            last_sent_title: String::new(),
            is_hidden: false,
            ime_preedit: String::new(),
            command_input: String::new(),
            command_selected: 0,
            command_panel_id: 0,
            command_parent: None,
            rename_old_title: String::new(),
            command_re_selected_title: String::new(),
            command_saved_selections: std::collections::HashMap::new(),
            command_panel_needs_scroll: false,
            cursor_visible: true,
            cursor_blink_start: 0.0,
        }
    }

    // Returns deduplicated base titles found in the vault (strips -N / -tmp suffixes).
    fn list_vault_titles(&self) -> Vec<String> {
        let Some(vault) = &self.state.vault_path else { return vec![] };
        let Ok(entries) = fs::read_dir(vault) else { return vec![] };
        let mut titles = std::collections::BTreeSet::new();
        for entry in entries.flatten() {
            let os_name = entry.file_name();
            let name = os_name.to_string_lossy();
            if !name.ends_with(".txt") { continue; }
            let stem = &name[..name.len() - 4];
            let base = if let Some(pos) = stem.rfind('-') {
                let suffix = &stem[pos + 1..];
                if suffix == "tmp" || suffix.chars().all(|c| c.is_ascii_digit()) {
                    &stem[..pos]
                } else { stem }
            } else { stem };
            if !base.is_empty() { titles.insert(base.to_string()); }
        }
        titles.into_iter().collect()
    }

    // Returns (display_name, full_path) for all files of a given base title.
    // -tmp first, then numbered files in reverse order (highest number first).
    fn list_vault_files_for_title(&self, title: &str) -> Vec<(String, String)> {
        let Some(vault) = &self.state.vault_path else { return vec![] };
        let Ok(entries) = fs::read_dir(vault) else { return vec![] };
        let prefix = format!("{}-", title);
        let mut tmp_file: Option<(String, String)> = None;
        let mut numbered: Vec<(u32, String, String)> = vec![];
        for entry in entries.flatten() {
            let path = entry.path();
            let os_name = entry.file_name();
            let name = os_name.to_string_lossy();
            if !name.ends_with(".txt") { continue; }
            let stem = &name[..name.len() - 4];
            if !stem.starts_with(prefix.as_str()) { continue; }
            let suffix = &stem[prefix.len()..];
            let display = stem.to_string();
            let path_str = path.to_string_lossy().to_string();
            if suffix == "tmp" {
                tmp_file = Some((display, path_str));
            } else if let Ok(n) = suffix.parse::<u32>() {
                numbered.push((n, display, path_str));
            }
        }
        numbered.sort_by(|a, b| b.0.cmp(&a.0));
        let mut result = vec![];
        if let Some(f) = tmp_file { result.push(f); }
        for (_, display, path) in numbered { result.push((display, path)); }
        result
    }

    fn save_state(&self) {
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.state) {
            fs::write(&self.config_path, json).ok();
        }
    }

    fn auto_save(&mut self) {
        if let Some(vault) = &self.state.vault_path {
            // Guard against overwriting existing tmp files with empty content
            if !self.state.current_title.is_empty() && !self.state.current_content.is_empty() {
                let tmp_path = vault.join(format!("{}-tmp.txt", self.state.current_title));
                fs::write(tmp_path, &self.state.current_content).ok();
                self.save_state();
            }
        }
    }

    fn load_tmp_file(&mut self) {
        if let Some(vault) = &self.state.vault_path {
            let tmp_path = vault.join(format!("{}-tmp.txt", self.state.current_title));
            if tmp_path.exists() {
                if let Ok(content) = fs::read_to_string(tmp_path) {
                    self.state.current_content = content;
                }
            }
        }
    }

    fn get_next_save_count(&self) -> u32 {
        if let Some(vault) = &self.state.vault_path {
            let mut count = 1;
            loop {
                let path = vault.join(format!("{}-{}.txt", self.state.current_title, count));
                if !path.exists() {
                    return count;
                }
                count += 1;
            }
        }
        1
    }

    fn save_final(&mut self) {
        if let Some(vault) = &self.state.vault_path {
            let count = self.get_next_save_count();
            let final_path = vault.join(format!("{}-{}.txt", self.state.current_title, count));
            fs::write(final_path, &self.state.current_content).ok();
            self.save_state();
        }
    }

    // Called when the global hotkey fires. Shows the window and resets state if it's a new day.
    // Because the hotkey belongs to THIS process, macOS grants activation rights here —
    // this is why the single-binary architecture works where the daemon approach did not.
    fn on_activate(&mut self, ctx: &egui::Context) {
        if self.is_hidden {
            let today = Local::now().format("%Y-%m-%d").to_string();
            let is_new_day = self.state.last_edit_date.as_ref() != Some(&today);
            let is_empty = self.state.current_title.is_empty();

            if (is_new_day || is_empty) && self.input_mode != InputMode::SelectVault {
                self.state.current_content = String::new();
                self.input_mode = InputMode::InputTitle;
            }

            self.is_hidden = false;
            self.request_focus = true;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    // Hide the window (keep process alive so the hotkey stays registered).
    fn hide(&mut self, ctx: &egui::Context) {
        self.auto_save();
        self.save_state(); // always persist state (including window_size) on hide
        self.is_hidden = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }
}

impl eframe::App for SeWriterApp {
    fn on_exit(&mut self) {
        self.save_state(); // persist window_size (and anything else) on Cmd+Q
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Cursor blink state machine — drives exactly 500 ms on / 500 ms off.
        // Using an explicit toggle avoids the uneven periods caused by `time % N`
        // (where repaint jitter can make a phase appear 1.5× longer than intended).
        let time = ctx.input(|i| i.time);
        {
            let elapsed = time - self.cursor_blink_start;
            if elapsed >= 0.5 {
                self.cursor_visible = !self.cursor_visible;
                self.cursor_blink_start += 0.5; // step forward, not snap to now — no drift
                // Catch-up: if we're more than one full period late, just reset.
                if time - self.cursor_blink_start >= 0.5 {
                    self.cursor_blink_start = time;
                }
            }
        }
        let remaining_blink = (0.5 - (time - self.cursor_blink_start)).max(0.016);

        // Runtime values from user config.
        let font_size = self.state.font_size;
        let cursor_height = font_size + 4.0;
        let line_height = font_size + 12.0;

        // Track window size in memory — flushed to disk in hide() and on_exit().
        // Comparing rounded values avoids spurious writes from sub-pixel fluctuations.
        if let Some(rect) = ctx.input(|i| i.viewport().inner_rect) {
            let size = [rect.width().round(), rect.height().round()];
            if self.state.window_size != Some(size) {
                self.state.window_size = Some(size);
            }
        }

        // Ctrl+W: show and activate window
        if HOTKEY_FIRED.swap(false, Ordering::Relaxed) {
            self.on_activate(ctx);
        }

        let title = if self.state.current_title.is_empty() {
            "SeWriter".to_string()
        } else {
            format!("SeWriter - {}", self.state.current_title)
        };
        if title != self.last_sent_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.last_sent_title = title;
        }

        if ctx.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command) {
            if self.input_mode == InputMode::EditContent && !self.state.current_content.is_empty() {
                self.show_save_dialog = true;
            }
        }

        // Cmd+W: hide window (auto-saves draft, process keeps running)
        if ctx.input(|i| i.key_pressed(egui::Key::W) && i.modifiers.command) && !self.show_save_dialog {
            self.hide(ctx);
        }

        // Cmd+Q: true quit
        if ctx.input(|i| i.key_pressed(egui::Key::Q) && i.modifiers.command) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // Cmd+/: open command panel (must be before the match so request_focus
        // is consumed by CommandPanel's TextEdit, not EditContent's).
        if self.input_mode == InputMode::EditContent
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Slash))
        {
            self.input_mode = InputMode::CommandPanel;
            self.command_input = "/".to_string();
            self.command_selected = 0;
            self.command_panel_id = self.command_panel_id.wrapping_add(1);
            self.request_focus = true;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            match self.input_mode {
                InputMode::SelectVault => {
                    ui.vertical_centered(|ui| {
                        ui.add_space(100.0);
                        ui.heading("Welcome to SeWriter");
                        ui.add_space(20.0);
                        ui.label("Please select a folder as your Vault");
                        ui.add_space(10.0);
                        if ui.button("Select Vault Folder").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                self.state.vault_path = Some(path.clone());
                                let guidebook = path.join("Guidebook.txt");
                                fs::write(&guidebook, "Welcome to SeWriter!\n\nThis is your guidebook.").ok();
                                self.state.current_title = "Guidebook".to_string();
                                self.state.current_content = fs::read_to_string(&guidebook).unwrap_or_default();
                                self.state.last_edit_date = Some(Local::now().format("%Y-%m-%d").to_string());
                                self.input_mode = InputMode::EditContent;
                                self.save_state();
                            }
                        }
                    });
                }
                InputMode::InputTitle => {
                    ui.add_space(20.0);
                    let te_out = egui::TextEdit::singleline(&mut self.state.current_title)
                        .hint_text("Title")
                        .font(egui::TextStyle::Heading)
                        .desired_width(f32::INFINITY)
                        .layouter(&mut |ui, text, wrap_width| {
                            let mut job = egui::text::LayoutJob::simple(
                                text.to_string(),
                                egui::FontId::proportional(TITLE_FONT_SIZE),
                                ui.visuals().text_color(),
                                wrap_width,
                            );
                            for section in &mut job.sections {
                                section.format.line_height = Some(TITLE_LINE_HEIGHT);
                            }
                            ui.fonts(|f| f.layout_job(job))
                        })
                        .show(ui);

                    let shift_y = (TITLE_LINE_HEIGHT - TITLE_FONT_SIZE) / 2.0;
                    let rounding = ui.style().interact(&te_out.response).rounding;

                    // Always cover egui's rendering so we can paint at the correct vertical position.
                    ui.painter().rect_filled(te_out.response.rect, rounding, ui.visuals().extreme_bg_color);

                    if self.state.current_title.is_empty() {
                        // Hint text centered in the row.
                        let hint_galley = ui.fonts(|f| f.layout_job(egui::text::LayoutJob::simple(
                            "Title".to_string(),
                            egui::FontId::proportional(TITLE_FONT_SIZE),
                            ui.visuals().weak_text_color(),
                            f32::INFINITY,
                        )));
                        ui.painter().galley(
                            te_out.galley_pos + egui::vec2(0.0, shift_y),
                            hint_galley,
                            ui.visuals().weak_text_color(),
                        );
                    } else {
                        // Selection
                        if let Some(ref cursor_range) = te_out.cursor_range {
                            if !cursor_range.is_empty() {
                                let [min_c, max_c] = cursor_range.sorted_cursors();
                                let rows = &te_out.galley.rows;
                                let last_row = rows.len().saturating_sub(1);
                                for ri in min_c.rcursor.row..=max_c.rcursor.row.min(last_row) {
                                    let row = &rows[ri];
                                    let left = if ri == min_c.rcursor.row { row.x_offset(min_c.rcursor.column) } else { row.rect.left() };
                                    let right = if ri == max_c.rcursor.row { row.x_offset(max_c.rcursor.column) } else { row.rect.right() };
                                    let center_y = te_out.galley_pos.y + row.rect.center().y;
                                    let sel_rect = egui::Rect::from_x_y_ranges(
                                        (te_out.galley_pos.x + left)..=(te_out.galley_pos.x + right),
                                        (center_y - TITLE_CURSOR_HEIGHT / 2.0)..=(center_y + TITLE_CURSOR_HEIGHT / 2.0),
                                    );
                                    ui.painter().rect_filled(sel_rect, 0.0, ui.visuals().selection.bg_fill);
                                }
                            }
                        }

                        // Clean galley shifted
                        let clean_galley = ui.fonts(|f| f.layout_job((*te_out.galley.job).clone()));
                        ui.painter().galley(
                            te_out.galley_pos + egui::vec2(0.0, shift_y),
                            clean_galley,
                            ui.visuals().text_color(),
                        );
                    }

                    // Cursor (always drawn manually since egui's cursor is hidden globally)
                    if te_out.response.has_focus() {
                        if let Some(ref cursor_range) = te_out.cursor_range {
                            let row_rect = te_out.galley.pos_from_cursor(&cursor_range.primary);
                            let screen_pos = egui::pos2(
                                te_out.galley_pos.x + row_rect.min.x,
                                te_out.galley_pos.y + row_rect.center().y,
                            );
                            let cursor_rect = egui::Rect::from_center_size(
                                screen_pos,
                                egui::vec2(1.0, TITLE_CURSOR_HEIGHT),
                            );
                            if self.cursor_visible {
                                ui.painter().rect_filled(cursor_rect, 0.0, ui.visuals().text_color());
                            }
                            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(remaining_blink));
                        }
                    }

                    if self.request_focus {
                        te_out.response.request_focus();
                        self.request_focus = false;
                    }

                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.state.current_title.is_empty() {
                        self.state.last_edit_date = Some(Local::now().format("%Y-%m-%d").to_string());
                        self.state.current_content = String::new();
                        self.load_tmp_file();
                        self.input_mode = InputMode::EditContent;
                        self.request_focus = true;
                        self.save_state();
                    }
                }
                InputMode::RenameTitle => {
                    // Esc: cancel, restore old title.
                    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                        self.state.current_title = self.rename_old_title.clone();
                        self.input_mode = InputMode::EditContent;
                        self.request_focus = true;
                    } else {
                        ui.add_space(20.0);
                        let te_out = egui::TextEdit::singleline(&mut self.state.current_title)
                            .hint_text("Title")
                            .font(egui::TextStyle::Heading)
                            .desired_width(f32::INFINITY)
                            .layouter(&mut |ui, text, wrap_width| {
                                let mut job = egui::text::LayoutJob::simple(
                                    text.to_string(),
                                    egui::FontId::proportional(TITLE_FONT_SIZE),
                                    ui.visuals().text_color(),
                                    wrap_width,
                                );
                                for section in &mut job.sections {
                                    section.format.line_height = Some(TITLE_LINE_HEIGHT);
                                }
                                ui.fonts(|f| f.layout_job(job))
                            })
                            .show(ui);

                        let shift_y = (TITLE_LINE_HEIGHT - TITLE_FONT_SIZE) / 2.0;
                        let rounding = ui.style().interact(&te_out.response).rounding;
                        ui.painter().rect_filled(te_out.response.rect, rounding, ui.visuals().extreme_bg_color);

                        if self.state.current_title.is_empty() {
                            let hint_galley = ui.fonts(|f| f.layout_job(egui::text::LayoutJob::simple(
                                "Title".to_string(), egui::FontId::proportional(TITLE_FONT_SIZE),
                                ui.visuals().weak_text_color(), f32::INFINITY,
                            )));
                            ui.painter().galley(
                                te_out.galley_pos + egui::vec2(0.0, shift_y),
                                hint_galley, ui.visuals().weak_text_color(),
                            );
                        } else {
                            if let Some(ref cursor_range) = te_out.cursor_range {
                                if !cursor_range.is_empty() {
                                    let [min_c, max_c] = cursor_range.sorted_cursors();
                                    let rows = &te_out.galley.rows;
                                    let last_row = rows.len().saturating_sub(1);
                                    for ri in min_c.rcursor.row..=max_c.rcursor.row.min(last_row) {
                                        let row = &rows[ri];
                                        let left = if ri == min_c.rcursor.row { row.x_offset(min_c.rcursor.column) } else { row.rect.left() };
                                        let right = if ri == max_c.rcursor.row { row.x_offset(max_c.rcursor.column) } else { row.rect.right() };
                                        let center_y = te_out.galley_pos.y + row.rect.center().y;
                                        let sel_rect = egui::Rect::from_x_y_ranges(
                                            (te_out.galley_pos.x + left)..=(te_out.galley_pos.x + right),
                                            (center_y - TITLE_CURSOR_HEIGHT / 2.0)..=(center_y + TITLE_CURSOR_HEIGHT / 2.0),
                                        );
                                        ui.painter().rect_filled(sel_rect, 0.0, ui.visuals().selection.bg_fill);
                                    }
                                }
                            }
                            let clean_galley = ui.fonts(|f| f.layout_job((*te_out.galley.job).clone()));
                            ui.painter().galley(
                                te_out.galley_pos + egui::vec2(0.0, shift_y),
                                clean_galley, ui.visuals().text_color(),
                            );
                        }

                        if te_out.response.has_focus() {
                            if let Some(ref cursor_range) = te_out.cursor_range {
                                let row_rect = te_out.galley.pos_from_cursor(&cursor_range.primary);
                                let screen_pos = egui::pos2(
                                    te_out.galley_pos.x + row_rect.min.x,
                                    te_out.galley_pos.y + row_rect.center().y,
                                );
                                let cursor_rect = egui::Rect::from_center_size(
                                    screen_pos, egui::vec2(1.0, TITLE_CURSOR_HEIGHT),
                                );
                                if self.cursor_visible {
                                    ui.painter().rect_filled(cursor_rect, 0.0, ui.visuals().text_color());
                                }
                                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(remaining_blink));
                            }
                        }

                        if self.request_focus {
                            te_out.response.request_focus();
                            self.request_focus = false;
                        }

                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.state.current_title.is_empty() {
                            // Rename the tmp file on disk if it exists.
                            if let Some(vault) = &self.state.vault_path {
                                let old_tmp = vault.join(format!("{}-tmp.txt", self.rename_old_title));
                                let new_tmp = vault.join(format!("{}-tmp.txt", self.state.current_title));
                                if old_tmp.exists() {
                                    fs::rename(&old_tmp, &new_tmp).ok();
                                }
                            }
                            self.input_mode = InputMode::EditContent;
                            self.request_focus = true;
                            self.save_state();
                        }
                    }
                }
                InputMode::EditContent => {
                    // IME backspace fix: on macOS Pinyin, pressing backspace when only
                    // one preedit char remains sends Ime(Disabled) instead of Preedit("").
                    // TextEdit's Disabled handler only sets ime_enabled=false — it never
                    // deletes the preedit text from content. The Preedit("") handler
                    // does call delete_selected() which correctly clears it, so we inject
                    // a synthetic Preedit("") immediately before Disabled to match the
                    // normal multi-char-preedit backspace behaviour (one press to delete).
                    // Note: Key::Backspace cannot be used here — egui removes it while
                    // ime_enabled is true via remove_ime_incompatible_events().
                    if !self.ime_preedit.is_empty() {
                        ctx.input_mut(|i| {
                            if let Some(pos) = i.events.iter().position(|e| {
                                matches!(e, egui::Event::Ime(egui::ImeEvent::Disabled))
                            }) {
                                i.events.insert(
                                    pos,
                                    egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
                                );
                            }
                        });
                    }

                    let scroll_out = egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            // Disable TextEdit while save dialog is open so that
                            // the Enter keypress confirming the dialog is not also
                            // inserted as a newline into the content.
                            if self.show_save_dialog {
                                ui.disable();
                            }

                            let te_out = egui::TextEdit::multiline(&mut self.state.current_content)
                                .desired_width(f32::INFINITY)
                                .layouter(&mut |ui, text, wrap_width| {
                                    let mut job = egui::text::LayoutJob::simple(
                                        text.to_string(),
                                        egui::FontId::proportional(font_size),
                                        ui.visuals().text_color(),
                                        wrap_width,
                                    );
                                    for section in &mut job.sections {
                                        section.format.line_height = Some(line_height);
                                    }
                                    ui.fonts(|f| f.layout_job(job))
                                })
                                .show(ui);

                            // Center text within each line_height row.
                            //
                            // epaint always places glyphs at the row top; the extra space from
                            // line_height accumulates at the bottom. shift_y shifts the galley
                            // down so glyphs are vertically centered. We use font_size (not
                            // row_height()) because row_height() includes line_gap (empty space
                            // below glyphs) which would under-shift.
                            //
                            // Selection highlights are baked into te_out.galley at full row
                            // height (0..line_height), which after shifting places them too low.
                            // Fix: draw selection rects manually at cursor_height centered on
                            // each row, then repaint a clean galley (no baked selection) shifted.
                            let shift_y = (line_height - font_size) / 2.0;
                            let rounding = ui.style().interact(&te_out.response).rounding;

                            // 1. Cover the original (unshifted) text.
                            ui.painter().rect_filled(
                                te_out.response.rect,
                                rounding,
                                ui.visuals().extreme_bg_color,
                            );

                            // 2. Draw selection rects behind text, centered on each row.
                            if let Some(ref cursor_range) = te_out.cursor_range {
                                if !cursor_range.is_empty() {
                                    let [min_c, max_c] = cursor_range.sorted_cursors();
                                    let min_rc = min_c.rcursor;
                                    let max_rc = max_c.rcursor;
                                    let sel_color = ui.visuals().selection.bg_fill;
                                    let rows = &te_out.galley.rows;
                                    let last_row = rows.len().saturating_sub(1);
                                    for ri in min_rc.row..=max_rc.row.min(last_row) {
                                        let row = &rows[ri];
                                        let left = if ri == min_rc.row {
                                            row.x_offset(min_rc.column)
                                        } else {
                                            row.rect.left()
                                        };
                                        let right = if ri == max_rc.row {
                                            row.x_offset(max_rc.column)
                                        } else {
                                            let extra = if row.ends_with_newline {
                                                row.height() / 2.0
                                            } else {
                                                0.0
                                            };
                                            row.rect.right() + extra
                                        };
                                        let center_y = te_out.galley_pos.y + row.rect.center().y;
                                        let sel_rect = egui::Rect::from_x_y_ranges(
                                            (te_out.galley_pos.x + left)
                                                ..=(te_out.galley_pos.x + right),
                                            (center_y - cursor_height / 2.0)
                                                ..=(center_y + cursor_height / 2.0),
                                        );
                                        ui.painter().rect_filled(sel_rect, 0.0, sel_color);
                                    }
                                }
                            }

                            // 3. Repaint a clean galley (no baked selection) at shifted pos.
                            let clean_galley = ui.fonts(|f| {
                                f.layout_job((*te_out.galley.job).clone())
                            });
                            ui.painter().galley(
                                te_out.galley_pos + egui::vec2(0.0, shift_y),
                                clean_galley,
                                ui.visuals().text_color(),
                            );

                            // 4. Draw cursor on top.
                            if te_out.response.has_focus() {
                                if let Some(ref cursor_range) = te_out.cursor_range {
                                    let row_rect = te_out.galley
                                        .pos_from_cursor(&cursor_range.primary);
                                    let screen_pos = egui::pos2(
                                        te_out.galley_pos.x + row_rect.min.x,
                                        te_out.galley_pos.y + row_rect.center().y,
                                    );
                                    let cursor_rect = egui::Rect::from_center_size(
                                        screen_pos,
                                        egui::vec2(1.0, cursor_height),
                                    );
                                    if self.cursor_visible {
                                        ui.painter().rect_filled(
                                            cursor_rect,
                                            0.0,
                                            ui.visuals().text_color(),
                                        );
                                    }
                                    ui.ctx().request_repaint_after(
                                        std::time::Duration::from_secs_f64(remaining_blink),
                                    );
                                }
                            }

                            te_out
                        });

                    // Track preedit content for the IME backspace fix above.
                    // Only update on explicit IME events — don't reset on idle frames
                    // (which have no Preedit event), or the fix won't see the preedit
                    // content when Ime(Disabled) arrives.
                    ctx.input(|i| {
                        for e in &i.events {
                            match e {
                                egui::Event::Ime(egui::ImeEvent::Preedit(s)) => {
                                    self.ime_preedit = s.clone();
                                }
                                egui::Event::Ime(egui::ImeEvent::Disabled)
                                | egui::Event::Ime(egui::ImeEvent::Commit(_)) => {
                                    self.ime_preedit = String::new();
                                }
                                _ => {}
                            }
                        }
                    });

                    let te_out = scroll_out.inner;
                    let response = &te_out.response;
                    if self.request_focus && !self.show_save_dialog {
                        response.request_focus();
                        self.request_focus = false;
                    }
                    if response.changed() {
                        self.auto_save();
                    }
                }
                InputMode::CommandPanel => {
                    // IME backspace fix (same as EditContent).
                    if !self.ime_preedit.is_empty() {
                        ctx.input_mut(|i| {
                            if let Some(pos) = i.events.iter().position(|e| {
                                matches!(e, egui::Event::Ime(egui::ImeEvent::Disabled))
                            }) {
                                i.events.insert(
                                    pos,
                                    egui::Event::Ime(egui::ImeEvent::Preedit(String::new())),
                                );
                            }
                        });
                    }

                    // Consume navigation/action keys before the TextEdit sees them.
                    let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
                    let up = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp));
                    let down = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown));
                    let enter = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
                    let backspace_when_empty = self.command_input.is_empty()
                        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace));

                    let parent = self.command_parent.clone();
                    if esc || backspace_when_empty {
                        match parent.as_deref() {
                            None => {
                                self.input_mode = InputMode::EditContent;
                                self.request_focus = true;
                            }
                            Some("/re-files") => {
                                // Level 3 → back to level 2; save /re-files pos, restore /re pos.
                                self.command_saved_selections.insert("/re-files".to_string(), self.command_selected);
                                let restored = self.command_saved_selections.get("/re").copied().unwrap_or(0);
                                self.command_parent = Some("/re".to_string());
                                self.command_input = String::new();
                                self.command_selected = restored;
                                self.command_panel_id = self.command_panel_id.wrapping_add(1);
                self.command_panel_needs_scroll = true;
                                self.request_focus = true;
                            }
                            Some("/config-font") => {
                                // /config-font → back to /config.
                                self.command_saved_selections.insert("/config-font".to_string(), self.command_selected);
                                let restored = self.command_saved_selections.get("/config").copied().unwrap_or(0);
                                self.command_parent = Some("/config".to_string());
                                self.command_input = String::new();
                                self.command_selected = restored;
                                self.command_panel_id = self.command_panel_id.wrapping_add(1);
                                self.command_panel_needs_scroll = true;
                                self.request_focus = true;
                            }
                            _ => {
                                // Any other sub-level → back to root; save current pos, restore root pos.
                                let key = parent.as_deref().unwrap_or("").to_string();
                                self.command_saved_selections.insert(key, self.command_selected);
                                let restored = self.command_saved_selections.get("").copied().unwrap_or(0);
                                self.command_parent = None;
                                self.command_input = "/".to_string();
                                self.command_selected = restored;
                                self.command_panel_id = self.command_panel_id.wrapping_add(1);
                self.command_panel_needs_scroll = true;
                                self.request_focus = true;
                            }
                        }
                    } else {
                        // Build dynamic lists for /re levels.
                        let vault_titles: Vec<String> = if parent.as_deref() == Some("/re") {
                            self.list_vault_titles()
                        } else { vec![] };
                        let vault_files: Vec<(String, String)> = if parent.as_deref() == Some("/re-files") {
                            let t = self.command_re_selected_title.clone();
                            self.list_vault_files_for_title(&t)
                        } else { vec![] };

                        // Build the filtered list: (display, key, desc).
                        let inp = &self.command_input;
                        let filtered: Vec<(String, String, String)> = match parent.as_deref() {
                            None => COMMANDS.iter()
                                .filter(|(n, _)| inp.is_empty() || n.starts_with(inp.as_str()))
                                .map(|&(n, d)| (n.to_string(), n.to_string(), d.to_string()))
                                .collect(),
                            Some("/vault") => VAULT_SUBCMDS.iter()
                                .filter(|(n, _)| inp.is_empty() || n.starts_with(inp.as_str()))
                                .map(|&(n, d)| (n.to_string(), n.to_string(), d.to_string()))
                                .collect(),
                            Some("/config") => vec![(
                                "font_size".to_string(),
                                "font_size".to_string(),
                                format!("current: {}px  (12–32)", self.state.font_size as u8),
                            )],
                            Some("/config-font") => (12u8..=32)
                                .map(|s| {
                                    let label = format!("{}px", s);
                                    (label.clone(), s.to_string(), String::new())
                                })
                                .collect(),
                            Some("/re") => vault_titles.iter()
                                .filter(|t| inp.is_empty() || t.starts_with(inp.as_str()))
                                .map(|t| (t.clone(), t.clone(), String::new()))
                                .collect(),
                            Some("/re-files") => vault_files.iter()
                                .filter(|(d, _)| inp.is_empty() || d.starts_with(inp.as_str()))
                                .map(|(d, p)| (d.clone(), p.clone(), String::new()))
                                .collect(),
                            _ => vec![],
                        };

                        // Clamp / update selection after typing.
                        if filtered.is_empty() {
                            self.command_selected = 0;
                        } else {
                            self.command_selected = self.command_selected.min(filtered.len() - 1);
                        }
                        if up && self.command_selected > 0 { self.command_selected -= 1; }
                        if down && !filtered.is_empty() && self.command_selected + 1 < filtered.len() {
                            self.command_selected += 1;
                        }

                        if enter {
                            if let Some((_, key, _)) = filtered.get(self.command_selected) {
                                let key = key.clone();
                                match (parent.as_deref(), key.as_str()) {
                                    (None, "/config") => {
                                        self.command_saved_selections.insert("".to_string(), self.command_selected);
                                        let restored = self.command_saved_selections.get("/config").copied().unwrap_or(0);
                                        self.command_parent = Some("/config".to_string());
                                        self.command_input = String::new();
                                        self.command_selected = restored;
                                        self.command_panel_id = self.command_panel_id.wrapping_add(1);
                                        self.command_panel_needs_scroll = true;
                                        self.request_focus = true;
                                    }
                                    (Some("/config"), "font_size") => {
                                        self.command_saved_selections.insert("/config".to_string(), self.command_selected);
                                        self.command_parent = Some("/config-font".to_string());
                                        self.command_input = String::new();
                                        // Pre-select the row matching the current font size.
                                        self.command_selected = (self.state.font_size as usize).saturating_sub(12).min(20);
                                        self.command_panel_id = self.command_panel_id.wrapping_add(1);
                                        self.command_panel_needs_scroll = true;
                                        self.request_focus = true;
                                    }
                                    (Some("/config-font"), size_str) => {
                                        if let Ok(size) = size_str.parse::<u8>() {
                                            self.state.font_size = (size as f32).clamp(12.0, 32.0);
                                            self.save_state();
                                        }
                                        self.command_parent = None;
                                        self.input_mode = InputMode::EditContent;
                                        self.request_focus = true;
                                    }
                                    (None, "/new") => {
                                        self.save_final();
                                        self.state.current_title = String::new();
                                        self.state.current_content = String::new();
                                        self.input_mode = InputMode::InputTitle;
                                        self.request_focus = true;
                                    }
                                    (None, "/finish") => {
                                        self.save_final();
                                        self.state.current_title = String::new();
                                        self.state.current_content = String::new();
                                        self.input_mode = InputMode::InputTitle;
                                        self.hide(ctx);
                                    }
                                    (None, "/title") => {
                                        self.rename_old_title = self.state.current_title.clone();
                                        self.input_mode = InputMode::RenameTitle;
                                        self.request_focus = true;
                                    }
                                    (None, "/vault") => {
                                        self.command_saved_selections.insert("".to_string(), self.command_selected);
                                        let restored = self.command_saved_selections.get("/vault").copied().unwrap_or(0);
                                        self.command_parent = Some("/vault".to_string());
                                        self.command_input = String::new();
                                        self.command_selected = restored;
                                        self.command_panel_id = self.command_panel_id.wrapping_add(1);
                self.command_panel_needs_scroll = true;
                                        self.request_focus = true;
                                    }
                                    (None, "/re") => {
                                        self.command_saved_selections.insert("".to_string(), self.command_selected);
                                        let restored = self.command_saved_selections.get("/re").copied().unwrap_or(0);
                                        self.command_parent = Some("/re".to_string());
                                        self.command_input = String::new();
                                        self.command_selected = restored;
                                        self.command_panel_id = self.command_panel_id.wrapping_add(1);
                self.command_panel_needs_scroll = true;
                                        self.request_focus = true;
                                    }
                                    (Some("/vault"), "new") => {
                                        if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                            self.state.vault_path = Some(path);
                                            self.save_state();
                                        }
                                        self.command_parent = None;
                                        self.input_mode = InputMode::EditContent;
                                        self.request_focus = true;
                                    }
                                    (Some("/vault"), "reset") => {
                                        if let Some(new_path) = rfd::FileDialog::new().pick_folder() {
                                            if let Some(old_path) = self.state.vault_path.clone() {
                                                if let Ok(entries) = fs::read_dir(&old_path) {
                                                    for entry in entries.flatten() {
                                                        let dst = new_path.join(entry.file_name());
                                                        fs::rename(entry.path(), dst).ok();
                                                    }
                                                }
                                            }
                                            self.state.vault_path = Some(new_path);
                                            self.save_state();
                                        }
                                        self.command_parent = None;
                                        self.input_mode = InputMode::EditContent;
                                        self.request_focus = true;
                                    }
                                    (Some("/re"), title) => {
                                        // Drill into file list for this title.
                                        self.command_saved_selections.insert("/re".to_string(), self.command_selected);
                                        let restored = self.command_saved_selections.get("/re-files").copied().unwrap_or(0);
                                        self.command_re_selected_title = title.to_string();
                                        self.command_parent = Some("/re-files".to_string());
                                        self.command_input = String::new();
                                        self.command_selected = restored;
                                        self.command_panel_id = self.command_panel_id.wrapping_add(1);
                self.command_panel_needs_scroll = true;
                                        self.request_focus = true;
                                    }
                                    (Some("/re-files"), path) => {
                                        // Open the selected file.
                                        if let Ok(content) = fs::read_to_string(&path) {
                                            self.state.current_content = content;
                                            self.state.current_title = self.command_re_selected_title.clone();
                                            self.state.last_edit_date = Some(Local::now().format("%Y-%m-%d").to_string());
                                            self.save_state();
                                        }
                                        self.command_parent = None;
                                        self.input_mode = InputMode::EditContent;
                                        self.request_focus = true;
                                    }
                                    _ => {}
                                }
                            }
                        }

                        // Render the panel only if we're still in CommandPanel mode.
                        if self.input_mode == InputMode::CommandPanel {
                            ui.add_space(20.0);

                            let hint_text = if self.command_parent.is_none() { "/" } else { "" };
                            let te_out = egui::TextEdit::singleline(&mut self.command_input)
                                .id_source(self.command_panel_id)
                                .font(egui::TextStyle::Body)
                                .desired_width(f32::INFINITY)
                                .cursor_at_end(true)
                                .layouter(&mut |ui, text, wrap_width| {
                                    let mut job = egui::text::LayoutJob::simple(
                                        text.to_string(),
                                        egui::FontId::proportional(font_size),
                                        ui.visuals().text_color(),
                                        wrap_width,
                                    );
                                    for section in &mut job.sections {
                                        section.format.line_height = Some(line_height);
                                    }
                                    ui.fonts(|f| f.layout_job(job))
                                })
                                .show(ui);

                            let shift_y = (line_height - font_size) / 2.0;
                            let rounding = ui.style().interact(&te_out.response).rounding;
                            ui.painter().rect_filled(te_out.response.rect, rounding, ui.visuals().extreme_bg_color);

                            if self.command_input.is_empty() && !hint_text.is_empty() {
                                let hint_galley = ui.fonts(|f| f.layout_job(egui::text::LayoutJob::simple(
                                    hint_text.to_string(),
                                    egui::FontId::proportional(font_size),
                                    ui.visuals().weak_text_color(),
                                    f32::INFINITY,
                                )));
                                ui.painter().galley(
                                    te_out.galley_pos + egui::vec2(0.0, shift_y),
                                    hint_galley,
                                    ui.visuals().weak_text_color(),
                                );
                            } else {
                                if let Some(ref cursor_range) = te_out.cursor_range {
                                    if !cursor_range.is_empty() {
                                        let [min_c, max_c] = cursor_range.sorted_cursors();
                                        let rows = &te_out.galley.rows;
                                        let last_row = rows.len().saturating_sub(1);
                                        for ri in min_c.rcursor.row..=max_c.rcursor.row.min(last_row) {
                                            let row = &rows[ri];
                                            let left = if ri == min_c.rcursor.row { row.x_offset(min_c.rcursor.column) } else { row.rect.left() };
                                            let right = if ri == max_c.rcursor.row { row.x_offset(max_c.rcursor.column) } else { row.rect.right() };
                                            let center_y = te_out.galley_pos.y + row.rect.center().y;
                                            let sel_rect = egui::Rect::from_x_y_ranges(
                                                (te_out.galley_pos.x + left)..=(te_out.galley_pos.x + right),
                                                (center_y - cursor_height / 2.0)..=(center_y + cursor_height / 2.0),
                                            );
                                            ui.painter().rect_filled(sel_rect, 0.0, ui.visuals().selection.bg_fill);
                                        }
                                    }
                                }
                                let clean_galley = ui.fonts(|f| f.layout_job((*te_out.galley.job).clone()));
                                ui.painter().galley(
                                    te_out.galley_pos + egui::vec2(0.0, shift_y),
                                    clean_galley,
                                    ui.visuals().text_color(),
                                );
                            }

                            // Cursor
                            if te_out.response.has_focus() {
                                if let Some(ref cursor_range) = te_out.cursor_range {
                                    let row_rect = te_out.galley.pos_from_cursor(&cursor_range.primary);
                                    let screen_pos = egui::pos2(
                                        te_out.galley_pos.x + row_rect.min.x,
                                        te_out.galley_pos.y + row_rect.center().y,
                                    );
                                    let cursor_rect = egui::Rect::from_center_size(
                                        screen_pos, egui::vec2(1.0, cursor_height),
                                    );
                                    if self.cursor_visible {
                                        ui.painter().rect_filled(cursor_rect, 0.0, ui.visuals().text_color());
                                    }
                                    ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(remaining_blink));
                                }
                            }

                            if self.request_focus {
                                te_out.response.request_focus();
                                self.request_focus = false;
                            }
                            if te_out.response.changed() {
                                self.command_selected = 0;
                            }

                            ctx.input(|i| {
                                for e in &i.events {
                                    match e {
                                        egui::Event::Ime(egui::ImeEvent::Preedit(s)) => {
                                            self.ime_preedit = s.clone();
                                        }
                                        egui::Event::Ime(egui::ImeEvent::Disabled)
                                        | egui::Event::Ime(egui::ImeEvent::Commit(_)) => {
                                            self.ime_preedit = String::new();
                                        }
                                        _ => {}
                                    }
                                }
                            });

                            ui.add_space(8.0);

                            let should_scroll = up || down || self.command_panel_needs_scroll;
                            self.command_panel_needs_scroll = false;

                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                let avail_width = ui.available_width();
                                let mut selected_rect: Option<egui::Rect> = None;
                                for (i, (display, _, desc)) in filtered.iter().enumerate() {
                                    let is_selected = i == self.command_selected;
                                    let (row_rect, _) = ui.allocate_exact_size(
                                        egui::vec2(avail_width, line_height), egui::Sense::hover(),
                                    );
                                    if is_selected {
                                        ui.painter().rect_filled(row_rect, 0.0, ui.visuals().selection.bg_fill);
                                        selected_rect = Some(row_rect);
                                    }
                                    let name_color = ui.visuals().text_color();
                                    let desc_color = if is_selected { ui.visuals().text_color() } else { ui.visuals().weak_text_color() };

                                    let name_galley = ui.fonts(|f| f.layout_job(egui::text::LayoutJob::simple(
                                        display.clone(), egui::FontId::proportional(font_size), name_color, f32::INFINITY,
                                    )));
                                    let name_width = name_galley.rect.width();
                                    ui.painter().galley(
                                        egui::pos2(row_rect.left() + 8.0, row_rect.top() + shift_y),
                                        name_galley, name_color,
                                    );
                                    let desc_galley = ui.fonts(|f| f.layout_job(egui::text::LayoutJob::simple(
                                        desc.to_string(), egui::FontId::proportional(font_size), desc_color, f32::INFINITY,
                                    )));
                                    ui.painter().galley(
                                        egui::pos2(row_rect.left() + 8.0 + name_width + 16.0, row_rect.top() + shift_y),
                                        desc_galley, desc_color,
                                    );
                                }
                                if should_scroll {
                                    if let Some(rect) = selected_rect {
                                        ui.scroll_to_rect(rect, Some(egui::Align::Center));
                                    }
                                }
                            });
                        }
                    }
                }
            }
        });

        if self.show_save_dialog {
            egui::Window::new("Save")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    let count = self.get_next_save_count();
                    let shift_y = (line_height - font_size) / 2.0;
                    let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));
                    let avail_w = ui.available_width();

                    // Labels: allocate each label's actual galley height (no fixed line_height)
                    // so wrapped text is never clipped.
                    let g1 = ui.fonts(|f| f.layout_job(egui::text::LayoutJob::simple(
                        format!("The file will be saved as {}-{}.txt", self.state.current_title, count),
                        egui::FontId::proportional(font_size), ui.visuals().text_color(), avail_w,
                    )));
                    let (r1, _) = ui.allocate_exact_size(egui::vec2(avail_w, g1.rect.height()), egui::Sense::hover());
                    ui.painter().galley(r1.min, g1, ui.visuals().text_color());

                    ui.add_space(4.0);

                    let g2 = ui.fonts(|f| f.layout_job(egui::text::LayoutJob::simple(
                        format!("Changes have been automatically saved as {}-tmp.txt", self.state.current_title),
                        egui::FontId::proportional(font_size), ui.visuals().text_color(), avail_w,
                    )));
                    let (r2, _) = ui.allocate_exact_size(egui::vec2(avail_w, g2.rect.height()), egui::Sense::hover());
                    ui.painter().galley(r2.min, g2, ui.visuals().text_color());

                    ui.add_space(8.0);

                    // Pre-measure buttons so we can center them horizontally.
                    let g_save = ui.fonts(|f| f.layout_job(egui::text::LayoutJob::simple(
                        "Save&Close".to_string(), egui::FontId::proportional(font_size),
                        egui::Color32::PLACEHOLDER, f32::INFINITY,
                    )));
                    let g_cancel = ui.fonts(|f| f.layout_job(egui::text::LayoutJob::simple(
                        "Cancel".to_string(), egui::FontId::proportional(font_size),
                        egui::Color32::PLACEHOLDER, f32::INFINITY,
                    )));
                    let save_w = g_save.rect.width() + 16.0;
                    let cancel_w = g_cancel.rect.width() + 16.0;
                    let btn_gap = 64.0;
                    let offset = ((avail_w - save_w - btn_gap - cancel_w) / 2.0).max(0.0);

                    ui.horizontal(|ui| {
                        ui.add_space(offset);

                        let (br_save, resp_save) = ui.allocate_exact_size(egui::vec2(save_w, line_height), egui::Sense::click());
                        let vis_save = ui.style().interact(&resp_save);
                        ui.painter().rect(br_save, vis_save.rounding, vis_save.bg_fill, vis_save.bg_stroke);
                        ui.painter().galley(egui::pos2(br_save.left() + 8.0, br_save.top() + shift_y), g_save, vis_save.fg_stroke.color);
                        if resp_save.clicked() || enter_pressed {
                            self.save_final();
                            self.show_save_dialog = false;
                            self.hide(ctx);
                        }

                        ui.add_space(btn_gap);

                        let (br_cancel, resp_cancel) = ui.allocate_exact_size(egui::vec2(cancel_w, line_height), egui::Sense::click());
                        let vis_cancel = ui.style().interact(&resp_cancel);
                        ui.painter().rect(br_cancel, vis_cancel.rounding, vis_cancel.bg_fill, vis_cancel.bg_stroke);
                        ui.painter().galley(egui::pos2(br_cancel.left() + 8.0, br_cancel.top() + shift_y), g_cancel, vis_cancel.fg_stroke.color);
                        if resp_cancel.clicked() || esc_pressed {
                            self.show_save_dialog = false;
                            self.request_focus = true;
                        }
                    });
                });
        }

        // Workaround for egui-winit 0.29 bug: set_ime_cursor_area uses ime.rect
        // (the full TextEdit widget bounds) instead of ime.cursor_rect (the actual cursor).
        ctx.output_mut(|out| {
            if let Some(ime) = out.ime.as_mut() {
                ime.rect = ime.cursor_rect;
            }
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    // Register global hotkey Ctrl+W. Must outlive run_native.
    let _hotkey_manager = GlobalHotKeyManager::new().expect("failed to init hotkey manager");
    let hotkey = HotKey::new(Some(Modifiers::CONTROL), Code::KeyW);
    _hotkey_manager.register(hotkey).expect("failed to register Ctrl+W");

    // Read saved window size before constructing NativeOptions so the window
    // opens at the same size the user left it. Falls back to 800×600.
    let saved_size = dirs::config_dir()
        .map(|p| p.join("sewriter").join("state.json"))
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<AppState>(&s).ok())
        .and_then(|s| s.window_size)
        .unwrap_or([800.0, 600.0]);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(saved_size)
            .with_min_inner_size([400.0, 300.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        "SeWriter",
        options,
        Box::new(move |cc| {
            let mut fonts = egui::FontDefinitions::default();
            if let Ok(font_data) = fs::read("/System/Library/Fonts/Hiragino Sans GB.ttc") {
                fonts.font_data.insert(
                    "cjk".to_owned(),
                    egui::FontData::from_owned(font_data),
                );
                fonts.families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "cjk".to_owned());
                fonts.families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .push("cjk".to_owned());
                cc.egui_ctx.set_fonts(fonts);
            }

            cc.egui_ctx.style_mut(|style| {
                style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
                style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(24.0));
                // Hide egui's built-in cursor; SeWriter draws its own at the correct height.
                // blink=false prevents egui from scheduling repaints for the invisible cursor.
                style.visuals.text_cursor.stroke = egui::Stroke::NONE;
                style.visuals.text_cursor.blink = false;
            });

            // Hotkey watcher: receives GlobalHotKeyEvent and wakes the egui event loop.
            // Since the hotkey belongs to this process, macOS grants activation rights
            // when we respond to it — unlike cross-process activation which is blocked in macOS 14+.
            let hotkey_ctx = cc.egui_ctx.clone();
            std::thread::Builder::new()
                .name("hotkey-watcher".to_string())
                .spawn(move || {
                    loop {
                        if GlobalHotKeyEvent::receiver().recv().is_ok() {
                            HOTKEY_FIRED.store(true, Ordering::Relaxed);
                            hotkey_ctx.request_repaint();
                        }
                    }
                })
                .expect("failed to spawn hotkey-watcher");

            Ok(Box::new(SeWriterApp::new()))
        }),
    )
}
