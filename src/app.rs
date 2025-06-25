use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use chrono::Local;
use eframe::egui::{self, Key, Response, Ui};
use egui::Margin;
use egui::text::CCursor;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::{Deserialize, Serialize};

use crate::{
    WINDOW_TITLE,
    completions_and_hints::{SapfDictionary, get_current_word_for_completion, get_word_at_cursor},
    window::custom_window_frame,
};

const STATE_FILE: &str = "sapf_apt_state.json";
const TEXT_EDIT_MARGIN: i8 = 10;
const DEFAULT_FONT_SIZE: f32 = 14.0;
const CHAR_WIDTH_RATIO: f32 = 0.6;
const LINE_HEIGHT_RATIO: f32 = 1.2;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Buffer {
    content: String,
    cursor_pos: usize,
    name: String,
    is_modified: bool,
    file_path: Option<PathBuf>,
}

impl Buffer {
    fn new(name: String) -> Self {
        Self {
            content: String::new(),
            cursor_pos: 0,
            name,
            is_modified: false,
            file_path: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct AppState {
    buffers: Vec<Buffer>,
    current_buffer_idx: usize,
    next_buffer_id: usize,
}

impl AppState {
    fn save_to_file(&self) -> Result<(), Box<dyn std::error::Error>> {
        let state_path = get_state_file_path()?;

        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let json_data = serde_json::to_string_pretty(self)?;
        fs::write(state_path, json_data)?;
        Ok(())
    }

    fn load_from_file() -> Result<Self, Box<dyn std::error::Error>> {
        let state_path = get_state_file_path()?;

        if !state_path.exists() {
            return Err("State file does not exist".into());
        }

        let json_data = fs::read_to_string(state_path)?;
        let state: AppState = serde_json::from_str(&json_data)?;
        Ok(state)
    }
}

fn get_state_file_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let mut path = dirs::config_dir()
        .or_else(dirs::home_dir)
        .ok_or("Could not find config or home directory")?;

    path.push("sapf-as-plain-text");
    path.push(STATE_FILE);
    Ok(path)
}

pub struct SapfAsPlainText {
    buffers: Vec<Buffer>,
    current_buffer_idx: usize,
    next_buffer_id: usize,
    from_sapf: String,
    from_sapf_receiver: Option<Receiver<String>>,
    pty_writer: Option<Box<dyn Write + Send>>,
    sapf_grammar: SapfDictionary,
    completions: Vec<crate::completions_and_hints::CompletionItem>,
    hover_info: Option<String>,
    show_completions: bool,
    should_focus_text_edit: bool,
    last_completion_cursor: Option<usize>,
    should_focus_completions: bool,
    show_buffer_bar: bool,
}

impl SapfAsPlainText {
    pub fn new() -> Self {
        let mut app = if let Ok(saved_state) = AppState::load_from_file() {
            Self::from_saved_state(saved_state)
        } else {
            Self::with_default_state()
        };

        app.run_sapf();
        app
    }

    fn with_default_state() -> Self {
        Self {
            buffers: vec![Buffer::new("Untitled 1".to_string())],
            current_buffer_idx: 0,
            next_buffer_id: 2,
            from_sapf: String::new(),
            from_sapf_receiver: None,
            pty_writer: None,
            sapf_grammar: SapfDictionary::new(),
            completions: Vec::new(),
            hover_info: None,
            show_completions: false,
            should_focus_text_edit: false,
            last_completion_cursor: None,
            should_focus_completions: false,
            show_buffer_bar: false,
        }
    }

    fn from_saved_state(state: AppState) -> Self {
        let buffer_count = state.buffers.len();
        Self {
            buffers: state.buffers,
            current_buffer_idx: state.current_buffer_idx.min(buffer_count.saturating_sub(1)),
            next_buffer_id: state.next_buffer_id,
            from_sapf: String::new(),
            from_sapf_receiver: None,
            pty_writer: None,
            sapf_grammar: SapfDictionary::new(),
            completions: Vec::new(),
            hover_info: None,
            show_completions: false,
            should_focus_text_edit: false,
            last_completion_cursor: None,
            should_focus_completions: false,
            show_buffer_bar: false,
        }
    }

    fn save_state(&self) {
        let app_state = AppState {
            buffers: self.buffers.clone(),
            current_buffer_idx: self.current_buffer_idx,
            next_buffer_id: self.next_buffer_id,
        };

        if let Err(e) = app_state.save_to_file() {
            eprintln!("Failed to save state: {}", e);
        }
    }

    fn export_current_buffer(&mut self) {
        let buffer_idx = self.current_buffer_idx;
        let content = self.buffers[buffer_idx].content.clone();
        let buffer_name = self.buffers[buffer_idx].name.clone();
        let file_path = self.buffers[buffer_idx].file_path.clone();
        let mut dialog = rfd::FileDialog::new()
            .set_title("Export Buffer As...")
            .add_filter("SAPF Files", &["sapf"])
            .add_filter("Text Files", &["txt"])
            .add_filter("All Files", &["*"]);

        if let Some(ref existing_path) = file_path {
            dialog = dialog.set_file_name(
                existing_path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("untitled.sapf"),
            );
            if let Some(parent) = existing_path.parent() {
                dialog = dialog.set_directory(parent);
            }
        } else {
            let default_filename = if buffer_name.ends_with(".sapf") {
                buffer_name.clone()
            } else {
                format!("{}.sapf", buffer_name)
            };
            dialog = dialog.set_file_name(&default_filename);
        }

        if let Some(path) = dialog.save_file() {
            match std::fs::write(&path, &content) {
                Ok(()) => {
                    let current_buffer = &mut self.buffers[buffer_idx];
                    current_buffer.file_path = Some(path.clone());
                    current_buffer.is_modified = false;

                    if current_buffer.name.starts_with("Untitled ") {
                        if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                            current_buffer.name = filename.to_string();
                        }
                    }

                    let final_name = current_buffer.name.clone();
                    self.save_state();
                    println!("Buffer '{}' saved to: {}", final_name, path.display());
                }
                Err(e) => {
                    eprintln!(
                        "Failed to save buffer '{}' to {}: {}",
                        buffer_name,
                        path.display(),
                        e
                    );
                }
            }
        }
    }

    fn load_file_into_new_buffer(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Open File...")
            .add_filter("SAPF Files", &["sapf"])
            .add_filter("Text Files", &["txt"])
            .add_filter("All Files", &["*"])
            .pick_file()
        {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    let filename = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Untitled")
                        .to_string();

                    let buffer = Buffer {
                        content,
                        cursor_pos: 0,
                        name: filename,
                        is_modified: false,
                        file_path: Some(path.clone()),
                    };

                    self.buffers.push(buffer);
                    self.current_buffer_idx = self.buffers.len() - 1;
                    self.should_focus_text_edit = true;
                    self.save_state();

                    println!("Loaded file: {}", path.display());
                }
                Err(e) => {
                    eprintln!("Failed to load file {}: {}", path.display(), e);
                }
            }
        }
    }

    fn get_current_buffer(&self) -> &Buffer {
        &self.buffers[self.current_buffer_idx]
    }

    fn get_current_buffer_mut(&mut self) -> &mut Buffer {
        &mut self.buffers[self.current_buffer_idx]
    }

    fn create_new_buffer(&mut self) {
        let buffer_name = format!("Untitled {}", self.next_buffer_id);
        self.buffers.push(Buffer::new(buffer_name));
        self.current_buffer_idx = self.buffers.len() - 1;
        self.next_buffer_id += 1;
        self.should_focus_text_edit = true;
        self.save_state();
    }

    fn close_current_buffer(&mut self) {
        if self.buffers.len() > 1 {
            self.buffers.remove(self.current_buffer_idx);
            if self.current_buffer_idx >= self.buffers.len() {
                self.current_buffer_idx = self.buffers.len() - 1;
            }
            self.should_focus_text_edit = true;
            self.save_state();
        }
    }

    fn switch_to_buffer(&mut self, idx: usize) {
        if idx < self.buffers.len() {
            self.current_buffer_idx = idx;
            self.should_focus_text_edit = true;
            self.save_state();
        }
    }

    fn next_buffer(&mut self) {
        if self.buffers.len() > 1 {
            self.current_buffer_idx = (self.current_buffer_idx + 1) % self.buffers.len();
            self.should_focus_text_edit = true;
            self.save_state();
        }
    }

    fn prev_buffer(&mut self) {
        if self.buffers.len() > 1 {
            self.current_buffer_idx = if self.current_buffer_idx == 0 {
                self.buffers.len() - 1
            } else {
                self.current_buffer_idx - 1
            };
            self.should_focus_text_edit = true;
            self.save_state();
        }
    }

    fn run_sapf(&mut self) {
        let pty = native_pty_system();

        let pty_pair = pty
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();

        let cmd = CommandBuilder::new("sapf");

        // not my terminology!
        let _ = pty_pair.slave.spawn_command(cmd).unwrap();
        let master = pty_pair.master;
        // ...

        let (output_sender, output_receiver) = mpsc::channel::<String>();
        let reader = master.try_clone_reader().unwrap();
        let writer = master.take_writer().unwrap();

        thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = line.trim_end().to_string();
                        if !trimmed.is_empty() {
                            eprintln!("{:?}", trimmed);
                            if output_sender.send(trimmed).is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Error reading from PTY: {}", e);
                        break;
                    }
                }
            }
        });

        self.pty_writer = Some(writer);
        self.from_sapf_receiver = Some(output_receiver);

        thread::sleep(Duration::from_millis(1000));
    }

    fn send_to_sapf(&mut self, code: &str) {
        if let Some(ref mut writer) = self.pty_writer {
            println!("Sending to SAPF: {}", code);
            if let Err(e) = writeln!(writer, "{}", code) {
                eprintln!("Failed to send to SAPF: {}", e);
            } else if let Err(e) = writer.flush() {
                eprintln!("Failed to flush PTY: {}", e);
            } else {
                println!("Sent: {}", code.trim());
            }
        } else {
            println!("SAPF not connected");
        }
    }

    fn update_output(&mut self) {
        if let Some(receiver) = &self.from_sapf_receiver {
            while let Ok(line) = receiver.try_recv() {
                if !line.trim().is_empty() {
                    self.from_sapf.push_str(&line);
                    self.from_sapf.push('\n');
                }
            }
        }
    }

    fn get_current_line(&self) -> String {
        let cursor_pos = self.get_current_buffer().cursor_pos;
        let lines: Vec<&str> = self.get_current_buffer().content.lines().collect();
        let mut char_count = 0;

        for line in &lines {
            let line_end = char_count + line.len();
            if cursor_pos >= char_count && cursor_pos <= line_end + 1 {
                return line.to_string();
            }
            char_count = line_end + 1;
        }

        if let Some(last_line) = lines.last() {
            return last_line.to_string();
        }

        self.get_current_buffer()
            .content
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .to_string()
    }

    fn handle_key_input(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if i.key_pressed(Key::Enter) && i.modifiers.ctrl {
                println!("{}", self.get_current_line());
                let code = self.get_current_line();
                if !code.trim().is_empty() {
                    self.send_to_sapf(&code);
                }
            }

            if i.key_pressed(Key::Period) && i.modifiers.ctrl {
                self.send_to_sapf("stop");
            }

            if i.key_pressed(Key::E) && i.modifiers.ctrl {
                self.send_to_sapf("stop");
                let code = self.get_current_line();
                if !code.trim().is_empty() {
                    self.send_to_sapf(&code);
                }
            }

            if i.key_pressed(Key::D) && i.modifiers.ctrl {
                self.send_to_sapf("clear");
            }

             if i.key_pressed(Key::P) && i.modifiers.ctrl {
                self.send_to_sapf("prstk");
            }

             if i.key_pressed(Key::R) && i.modifiers.ctrl {
                let code = self.get_current_line();
                let date = Local::now().format("%Y-%m-%d_%H-%M-%S").to_string();
                let buffer_name = &self.buffers[self.current_buffer_idx].name;
                let file_name = format!("{}-{}", buffer_name, date);
                let combined = format!("{} \"{}\" record", code, file_name);
                self.send_to_sapf(&combined);
            }

            if i.key_pressed(Key::Tab) && i.modifiers.ctrl {
                self.trigger_completions();
                self.should_focus_completions = true;
            }

            if i.key_pressed(Key::T) && i.modifiers.ctrl {
                self.create_new_buffer();
            }

            if i.key_pressed(Key::W) && i.modifiers.ctrl {
                self.close_current_buffer();
            }

            if i.key_pressed(Key::S) && i.modifiers.ctrl {
                self.export_current_buffer();
            }

            if i.key_pressed(Key::O) && i.modifiers.ctrl {
                self.load_file_into_new_buffer();
            }

            if i.key_pressed(Key::Tab) && i.modifiers.alt {
                self.next_buffer();
            }

            if i.key_pressed(Key::Tab) && i.modifiers.shift && i.modifiers.alt {
                self.prev_buffer();
            }
        });
    }

    fn trigger_completions(&mut self) {
        if let Some(current_word) = get_current_word_for_completion(
            &self.get_current_buffer().content,
            self.get_current_buffer().cursor_pos,
        ) {
            if !current_word.is_empty() {
                self.completions = self.sapf_grammar.get_completions(&current_word);
                self.show_completions = !self.completions.is_empty();
            } else {
                self.completions = self.sapf_grammar.get_completions("");
                self.show_completions = !self.completions.is_empty();
            }
        } else {
            self.completions = self.sapf_grammar.get_completions("");
            self.show_completions = !self.completions.is_empty();
        }
    }

    fn update_completions_and_hints(&mut self) {
        if let Some((word, _, _)) = get_word_at_cursor(
            &self.get_current_buffer().content,
            self.get_current_buffer().cursor_pos,
        ) {
            self.hover_info = self.sapf_grammar.get_hover_info(&word);
        } else {
            self.hover_info = None;
        }
    }

    fn show_completion_popup(&mut self, ui: &mut Ui, text_response: &Response) {
        if self.show_completions && !self.completions.is_empty() {
            let popup_pos = if let Some(cursor_pos) = self.get_cursor_screen_pos(ui, text_response)
            {
                cursor_pos + egui::vec2(0.0, 20.0)
            } else {
                text_response.rect.left_bottom() + egui::vec2(0.0, 5.0)
            };

            egui::Area::new(egui::Id::new("completion_popup"))
                .fixed_pos(popup_pos)
                .show(ui.ctx(), |ui| {
                    egui::Frame::new()
                        .corner_radius(5.0)
                        .inner_margin(5.0)
                        .show(ui, |ui| {
                            ui.set_max_width(300.0);

                            // TODO This is very stupid and inelegant
                            let mut selected_completion: Option<String> = None;
                            for (i, item) in self.completions.iter().enumerate() {
                                if i >= 10 {
                                    break;
                                }
                                let response = ui.selectable_label(false, &item.label);
                                if i == 0 && self.should_focus_completions {
                                    response.request_focus();
                                    self.should_focus_completions = false;
                                }
                                if response.has_focus() {
                                    self.hover_info = Some(item.documentation.clone());
                                    egui::Area::new(egui::Id::new("docs"))
                                        .fixed_pos(popup_pos + egui::vec2(80.0, 0.0))
                                        .show(ui.ctx(), |ui| {
                                            egui::Frame::new()
                                                .corner_radius(5.0)
                                                .inner_margin(5.0)
                                                .show(ui, |ui| {
                                                    ui.set_max_width(300.0);
                                                    ui.label(&item.documentation);
                                                });
                                        });
                                }
                                if response.clicked() {
                                    selected_completion = Some(item.label.clone());
                                }

                                if response.lost_focus() {
                                    self.should_focus_text_edit = true;
                                    self.show_completions = false;
                                }
                            }

                            if let Some(completion) = selected_completion {
                                self.apply_completion(&completion);
                                self.show_completions = false;
                                self.last_completion_cursor =
                                    Some(self.get_current_buffer().cursor_pos);
                                self.should_focus_text_edit = true;
                            }
                        });
                });
        }
    }

    fn get_cursor_screen_pos(&self, ui: &Ui, text_response: &Response) -> Option<egui::Pos2> {
        let id = text_response.id;
        let state = egui::TextEdit::load_state(ui.ctx(), id)?;
        let cursor_range = state.cursor.char_range()?;
        let cursor_index = cursor_range.primary.index;

        let text_rect = text_response.rect;
        let margin = TEXT_EDIT_MARGIN as f32;
        let content_rect = text_rect.shrink(margin);

        let text_to_cursor = if cursor_index <= self.get_current_buffer().content.len() {
            &self.get_current_buffer().content[..cursor_index]
        } else {
            &self.get_current_buffer().content
        };

        let current_line_index = text_to_cursor.lines().count().saturating_sub(1);
        let current_line = text_to_cursor.lines().last().unwrap_or("");

        let font_size = ui
            .style()
            .text_styles
            .get(&egui::TextStyle::Body)
            .map(|font| font.size)
            .unwrap_or(DEFAULT_FONT_SIZE);

        let char_width = font_size * CHAR_WIDTH_RATIO;
        let line_height = font_size * LINE_HEIGHT_RATIO;

        let cursor_x = content_rect.left() + (current_line.chars().count() as f32 * char_width);
        let cursor_y = content_rect.top() + (current_line_index as f32 * line_height);

        Some(egui::pos2(
            cursor_x.min(content_rect.right()),
            cursor_y.min(content_rect.bottom()),
        ))
    }

    fn apply_completion(&mut self, completion: &str) {
        let cursor_pos = self.get_current_buffer().cursor_pos;
        let input = &self.get_current_buffer().content;
        let mut word_start = cursor_pos;
        let bytes = input.as_bytes();
        while word_start > 0 {
            let c = bytes[word_start - 1];
            if !c.is_ascii_alphanumeric() && c != b'_' && c != b'.' {
                break;
            }
            word_start -= 1;
        }

        let mut new_input = String::new();
        new_input.push_str(&input[..word_start]);
        new_input.push_str(completion);
        new_input.push_str(&input[cursor_pos..]);
        let new_cursor_pos = word_start + completion.len();

        self.get_current_buffer_mut().content = new_input;
        self.get_current_buffer_mut().cursor_pos = new_cursor_pos;
    }
}

impl eframe::App for SapfAsPlainText {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_state();
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.update_output();
        self.handle_key_input(ctx);
        self.update_completions_and_hints();
        let hover_info = self.hover_info.clone().unwrap_or_default();

        custom_window_frame(ctx, WINDOW_TITLE, |ui| {
            egui::TopBottomPanel::bottom("console")
                .show_separator_line(false)
                .resizable(true)
                .exact_height(180.0)
                .show_inside(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(hover_info);
                        ui.add_space(10.0);
                        egui::ScrollArea::vertical()
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(&mut self.from_sapf)
                                        .desired_width(ui.available_width())
                                        .margin(Margin::same(TEXT_EDIT_MARGIN))
                                        .frame(false)
                                        .interactive(false),
                                );
                            });
                    });
                });

            egui::CentralPanel::default().show_inside(ui, |ui| {
                let pointer_pos = ui.ctx().pointer_latest_pos();
                let top_hover_height = 40.0;

                if let Some(pos) = pointer_pos {
                    let ui_rect = ui.max_rect();
                    self.show_buffer_bar = pos.y <= ui_rect.min.y + top_hover_height;
                }

                let mut switch_to_buffer: Option<usize> = None;
                let mut create_new = false;
                let mut close_current = false;
                let mut export_buffer = false;
                let mut load_file = false;

                if self.show_buffer_bar {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            if ui.small_button("add").clicked() {
                                create_new = true;
                            }

                            if ui.button("close").clicked() && self.buffers.len() > 1 {
                                close_current = true;
                            }

                            ui.separator();

                            if ui.button("open").clicked() {
                                load_file = true;
                            }

                            if ui.button("export").clicked() {
                                export_buffer = true;
                            }
                            ui.separator();
                        });

                        ui.add_space(5.0);

                        egui::ScrollArea::horizontal()
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;

                                    for (idx, buffer) in self.buffers.iter().enumerate() {
                                        let label = if buffer.is_modified {
                                            format!("{} *", buffer.name)
                                        } else {
                                            buffer.name.clone()
                                        };

                                        let is_current = idx == self.current_buffer_idx;
                                        if ui.selectable_label(is_current, &label).clicked() {
                                            switch_to_buffer = Some(idx);
                                        }
                                    }
                                    ui.separator();
                                });
                            });
                    });

                    ui.separator();
                } else {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        ui.style_mut().visuals.widgets.inactive.weak_bg_fill =
                            egui::Color32::TRANSPARENT;

                        ui.label("•");
                        ui.label(&self.buffers[self.current_buffer_idx].name);
                        if self.buffers[self.current_buffer_idx].is_modified {
                            ui.colored_label(egui::Color32::LIGHT_YELLOW, "*");
                        }
                    });
                    ui.add_space(2.0);
                }

                if let Some(idx) = switch_to_buffer {
                    self.switch_to_buffer(idx);
                }
                if create_new {
                    self.create_new_buffer();
                }
                if close_current {
                    self.close_current_buffer();
                }
                if export_buffer {
                    self.export_current_buffer();
                }
                if load_file {
                    self.load_file_into_new_buffer();
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let input = ui.add(
                        egui::TextEdit::multiline(&mut self.get_current_buffer_mut().content)
                            .desired_width(ui.available_width())
                            .desired_rows(35)
                            .margin(Margin::same(TEXT_EDIT_MARGIN))
                            .frame(false),
                    );

                    if self.should_focus_text_edit {
                        input.request_focus();
                        self.should_focus_text_edit = false;
                    }

                    if input.changed() || input.has_focus() {
                        let id = input.id;
                        if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                            if self.last_completion_cursor.is_some() {
                                state
                                    .cursor
                                    .set_char_range(Some(egui::text::CCursorRange::one(
                                        CCursor::new(self.get_current_buffer().cursor_pos),
                                    )));
                                state.store(ui.ctx(), id);
                                self.last_completion_cursor = None;
                            } else if let Some(cursor_range) = state.cursor.char_range() {
                                self.get_current_buffer_mut().cursor_pos =
                                    cursor_range.primary.index;
                                self.get_current_buffer_mut().is_modified = true;
                            }
                        }
                    }

                    if input.changed() {
                        self.save_state();
                    }

                    self.show_completion_popup(ui, &input);
                });
            });
        });
    }
}
