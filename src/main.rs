mod dict;
mod completions_and_hints;
mod window;
use std::{
    io::{BufRead, BufReader, Write},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

use eframe::egui::{self, Key, Response, Ui};
use egui::text::CCursor;
use egui::{Margin, Vec2, vec2};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::{
    completions_and_hints::{SapfDictionary, get_current_word_for_completion, get_word_at_cursor},
    window::custom_window_frame,
};

const WINDOW_SIZE: Vec2 = vec2(680.0, 840.0);
const TEXT_EDIT_MARGIN: i8 = 10;
const WINDOW_TITLE: &str = "sapf as plain* text";
const DEFAULT_FONT_SIZE: f32 = 14.0;
const CHAR_WIDTH_RATIO: f32 = 0.6;
const LINE_HEIGHT_RATIO: f32 = 1.2;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size(WINDOW_SIZE)
            .with_transparent(true),

        ..Default::default()
    };
    eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(|_| Ok(Box::new(SapfAsPlainText::new()))),
    )
}

struct SapfAsPlainText {
    input: String,
    from_sapf: String,
    from_sapf_receiver: Option<Receiver<String>>,
    pty_writer: Option<Box<dyn Write + Send>>,
    cursor_pos: usize,
    sapf_grammar: SapfDictionary,
    completions: Vec<crate::completions_and_hints::CompletionItem>,
    hover_info: Option<String>,
    show_completions: bool,
    should_focus_text_edit: bool,
    last_completion_cursor: Option<usize>,
    should_focus_completions: bool,
}

impl SapfAsPlainText {
    pub fn new() -> Self {
        let mut app = Self {
            input: String::new(),
            from_sapf: String::new(),
            from_sapf_receiver: None,
            pty_writer: None,
            cursor_pos: 0,
            sapf_grammar: SapfDictionary::new(),
            completions: Vec::new(),
            hover_info: None,
            show_completions: false,
            should_focus_text_edit: false,
            last_completion_cursor: None,
            should_focus_completions: false,
        };

        app.run_sapf();
        app
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
                    Ok(0) => break, // EOF
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
        let cursor_pos = self.cursor_pos;
        let lines: Vec<&str> = self.input.lines().collect();
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

        self.input
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

            if i.key_pressed(Key::Tab) && i.modifiers.ctrl {
                self.trigger_completions();
                self.should_focus_completions = true;
            }
        });
    }

    fn trigger_completions(&mut self) {
        if let Some(current_word) = get_current_word_for_completion(&self.input, self.cursor_pos) {
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
        if let Some((word, _, _)) = get_word_at_cursor(&self.input, self.cursor_pos) {
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
                                self.last_completion_cursor = Some(self.cursor_pos);
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

        let text_to_cursor = if cursor_index <= self.input.len() {
            &self.input[..cursor_index]
        } else {
            &self.input
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
        let cursor_pos = self.cursor_pos;
        let input = &self.input;
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

        self.input = new_input;
        self.cursor_pos = new_cursor_pos;
    }
}

impl eframe::App for SapfAsPlainText {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        egui::Rgba::TRANSPARENT.to_array()
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
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let input = ui.add(
                        egui::TextEdit::multiline(&mut self.input)
                            .desired_width(ui.available_width())
                            .desired_rows(35)
                            .margin(Margin::same(TEXT_EDIT_MARGIN))
                            // .code_editor()
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
                                        CCursor::new(self.cursor_pos),
                                    )));
                                state.store(ui.ctx(), id);
                                self.last_completion_cursor = None;
                            } else if let Some(cursor_range) = state.cursor.char_range() {
                                self.cursor_pos = cursor_range.primary.index;
                            }
                        }
                    }

                    self.show_completion_popup(ui, &input);
                });
            });
        });
    }
}
