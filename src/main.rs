use eframe::egui;
use chrono::Local;
use global_hotkey::{GlobalHotKeyManager, hotkey::{Code, HotKey, Modifiers}, GlobalHotKeyEvent};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

static HOTKEY_FIRED: AtomicBool = AtomicBool::new(false);

// Text and cursor layout constants (will be user-configurable in a future release).
const FONT_SIZE: f32 = 16.0;
const CURSOR_HEIGHT: f32 = FONT_SIZE + 4.0;   // slightly taller than the text glyphs
const LINE_HEIGHT: f32 = CURSOR_HEIGHT + 8.0;  // 4 px of equal padding around the cursor

#[derive(Serialize, Deserialize, Default)]
struct AppState {
    vault_path: Option<PathBuf>,
    current_title: String,
    current_content: String,
    last_edit_date: Option<String>,
    window_size: Option<[f32; 2]>,
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
}

#[derive(PartialEq)]
enum InputMode {
    SelectVault,
    InputTitle,
    EditContent,
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
        }
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
                    ui.vertical_centered(|ui| {
                        ui.add_space(200.0);
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.state.current_title)
                                .hint_text("title")
                                .font(egui::TextStyle::Heading)
                        );

                        if self.request_focus {
                            response.request_focus();
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
                    });
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
                                        egui::FontId::proportional(FONT_SIZE),
                                        ui.visuals().text_color(),
                                        wrap_width,
                                    );
                                    for section in &mut job.sections {
                                        section.format.line_height = Some(LINE_HEIGHT);
                                    }
                                    ui.fonts(|f| f.layout_job(job))
                                })
                                .show(ui);

                            // Center text within each LINE_HEIGHT row.
                            //
                            // epaint always places glyphs at the row top; the extra space from
                            // LINE_HEIGHT accumulates at the bottom. shift_y shifts the galley
                            // down so glyphs are vertically centered. We use FONT_SIZE (not
                            // row_height()) because row_height() includes line_gap (empty space
                            // below glyphs) which would under-shift.
                            //
                            // Selection highlights are baked into te_out.galley at full row
                            // height (0..LINE_HEIGHT), which after shifting places them too low.
                            // Fix: draw selection rects manually at CURSOR_HEIGHT centered on
                            // each row, then repaint a clean galley (no baked selection) shifted.
                            let shift_y = (LINE_HEIGHT - FONT_SIZE) / 2.0;
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
                                            (center_y - CURSOR_HEIGHT / 2.0)
                                                ..=(center_y + CURSOR_HEIGHT / 2.0),
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
                                        egui::vec2(2.0, CURSOR_HEIGHT),
                                    );
                                    let time = ui.input(|i| i.time);
                                    if (time % 2.0) < 1.0 {
                                        ui.painter().rect_filled(
                                            cursor_rect,
                                            0.0,
                                            ui.visuals().text_color(),
                                        );
                                    }
                                    ui.ctx().request_repaint_after(
                                        std::time::Duration::from_millis(1000),
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
            }
        });

        if self.show_save_dialog {
            egui::Window::new("Save")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    let count = self.get_next_save_count();
                    ui.label(format!("The file will be saved as {}-{}.txt", self.state.current_title, count));
                    ui.label(format!("Changes have been automatically saved as {}-tmp.txt", self.state.current_title));
                    ui.add_space(10.0);

                    let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let esc_pressed = ui.input(|i| i.key_pressed(egui::Key::Escape));

                    ui.horizontal(|ui| {
                        if ui.button("Save and close").clicked() || enter_pressed {
                            self.save_final();
                            self.show_save_dialog = false;
                            self.hide(ctx);
                        }
                        if ui.button("Cancel").clicked() || esc_pressed {
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
                style.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(FONT_SIZE));
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
