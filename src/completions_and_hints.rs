use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::dict::VALUES_JSON;

//TODO

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct CategoryData {
    pub description: String,
    pub items: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub documentation: String,
}

pub struct SapfDictionary {
    categories: HashMap<String, CategoryData>,
    all_keywords: HashMap<String, String>,
}

impl SapfDictionary {
    pub fn new() -> Self {
        let categories = load_categories();
        let all_keywords = Self::build_all_keywords(&categories);

        Self {
            categories,
            all_keywords,
        }
    }

    fn build_all_keywords(categories: &HashMap<String, CategoryData>) -> HashMap<String, String> {
        let mut all_keywords = HashMap::new();
        for category in categories.values() {
            for (k, v) in &category.items {
                all_keywords.insert(k.clone(), v.clone());
            }
        }
        all_keywords
    }

    pub fn get_completions(&self, current_input: &str) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        if let Some((category_prefix, item_prefix)) = current_input.split_once('.') {
            if let Some(category) = self.categories.get(category_prefix) {
                items.extend(
                    category
                        .items
                        .iter()
                        .filter(|(k, _)| k.starts_with(item_prefix.trim()))
                        .map(|(k, d)| CompletionItem {
                            label: k.clone(),
                            documentation: d.clone(),
                        }),
                );
            }
        } else {
            for (category_name, category_data) in &self.categories {
                if category_name.starts_with(current_input) {
                    items.push(CompletionItem {
                        label: format!("{}.", category_name),
                        documentation: category_data.description.clone(),
                    });
                }
            }

            items.extend(
                self.all_keywords
                    .iter()
                    .filter(|(k, _)| k.starts_with(current_input))
                    .map(|(k, d)| CompletionItem {
                        label: k.clone(),
                        documentation: d.clone(),
                    }),
            );
        }

        items
    }

    pub fn get_hover_info(&self, word: &str) -> Option<String> {
        if let Some(category) = self.categories.get(word) {
            return Some(category.description.clone());
        }

        if let Some(doc) = self.all_keywords.get(word) {
            return Some(doc.clone());
        }

        None
    }
}

fn load_categories() -> HashMap<String, CategoryData> {
    serde_json::from_str(VALUES_JSON).expect("Failed to parse SAPF categories JSON")
}

pub fn get_word_at_cursor(text: &str, cursor_pos: usize) -> Option<(String, usize, usize)> {
    if cursor_pos > text.len() {
        return None;
    }

    let bytes = text.as_bytes();
    let mut start = cursor_pos;
    let mut end = cursor_pos;

    while start > 0 && is_word_char(bytes[start - 1]) {
        start -= 1;
    }

    while end < bytes.len() && is_word_char(bytes[end]) {
        end += 1;
    }

    if start < end {
        Some((text[start..end].to_string(), start, end))
    } else {
        None
    }
}

pub fn get_current_word_for_completion(text: &str, cursor_pos: usize) -> Option<String> {
    if cursor_pos > text.len() {
        return None;
    }

    let bytes = text.as_bytes();
    let mut start = cursor_pos;

    while start > 0 {
        let c = bytes[start - 1];
        if !is_word_char(c) && c != b'.' {
            break;
        }
        start -= 1;
    }

    if start < cursor_pos {
        Some(text[start..cursor_pos].to_string())
    } else {
        None
    }
}

fn is_word_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}
