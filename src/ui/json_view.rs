//! Pretty-print + light syntax highlight for sticky Value JSON payloads.

use eframe::egui::{self, Color32, FontId, TextFormat};
use eframe::epaint::text::{LayoutJob, LayoutSection};

/// Keys / structure punctuation on dark chrome.
const C_KEY: Color32 = Color32::from_rgb(140, 190, 255);
const C_STRING: Color32 = Color32::from_rgb(180, 220, 150);
const C_NUMBER: Color32 = Color32::from_rgb(230, 190, 120);
const C_BOOL: Color32 = Color32::from_rgb(120, 200, 230);
const C_NULL: Color32 = Color32::from_rgb(150, 150, 160);
const C_PUNCT: Color32 = Color32::from_rgb(170, 176, 188);
const C_DEFAULT: Color32 = Color32::from_rgb(220, 222, 228);

/// If `payload` is a JSON object or array, return pretty-printed text.
pub fn try_pretty_json(payload: &str) -> Option<String> {
    let t = payload.trim();
    if !(t.starts_with('{') || t.starts_with('[')) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(t).ok()?;
    if !value.is_object() && !value.is_array() {
        return None;
    }
    serde_json::to_string_pretty(&value).ok()
}

/// Draw pretty JSON with simple syntax colors inside a scroll area.
pub fn draw_pretty(ui: &mut egui::Ui, pretty: &str) {
    ui.label(egui::RichText::new("JSON").small().weak());
    egui::ScrollArea::vertical()
        .max_height(220.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            let job = highlight_job(pretty);
            ui.add(egui::Label::new(job).selectable(true));
        });
}

/// Tokenize pretty JSON into a colored `LayoutJob` (keys, strings, numbers, bools, null).
fn highlight_job(src: &str) -> LayoutJob {
    let font = FontId::monospace(12.0);
    let mut job = LayoutJob {
        text: src.to_string(),
        break_on_newline: true,
        ..Default::default()
    };

    let bytes = src.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let start = i;
        let b = bytes[i];

        if b.is_ascii_whitespace() {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            push(&mut job, start, i, C_DEFAULT, font.clone());
            continue;
        }

        // String (key if followed by ':')
        if b == b'"' {
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == b'"' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            let mut j = i;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let color = if j < bytes.len() && bytes[j] == b':' {
                C_KEY
            } else {
                C_STRING
            };
            push(&mut job, start, i, color, font.clone());
            continue;
        }

        if matches!(b, b'{' | b'}' | b'[' | b']' | b':' | b',') {
            i += 1;
            push(&mut job, start, i, C_PUNCT, font.clone());
            continue;
        }

        if b == b'-' || b.is_ascii_digit() {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_digit()
                    || matches!(bytes[i], b'.' | b'e' | b'E' | b'+' | b'-'))
            {
                i += 1;
            }
            push(&mut job, start, i, C_NUMBER, font.clone());
            continue;
        }

        if b.is_ascii_alphabetic() {
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let word = &src[start..i];
            let color = match word {
                "true" | "false" => C_BOOL,
                "null" => C_NULL,
                _ => C_DEFAULT,
            };
            push(&mut job, start, i, color, font.clone());
            continue;
        }

        i += 1;
        push(&mut job, start, i, C_DEFAULT, font.clone());
    }

    job
}

fn push(job: &mut LayoutJob, start: usize, end: usize, color: Color32, font: FontId) {
    if start >= end {
        return;
    }
    job.sections.push(LayoutSection {
        leading_space: 0.0,
        byte_range: start..end,
        format: TextFormat {
            font_id: font,
            color,
            valign: egui::Align::Center,
            ..Default::default()
        },
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_object() {
        let p = try_pretty_json(r#"{"a":1,"b":true}"#).unwrap();
        assert!(p.contains('\n'));
        assert!(p.contains("\"a\""));
    }

    #[test]
    fn reject_plain_text() {
        assert!(try_pretty_json("hello").is_none());
        assert!(try_pretty_json("42").is_none());
    }

    #[test]
    fn highlight_covers_all_bytes() {
        let src = "{\n  \"ok\": true,\n  \"n\": 2.5\n}";
        let job = highlight_job(src);
        let covered: usize = job.sections.iter().map(|s| s.byte_range.len()).sum();
        assert_eq!(covered, src.len());
        assert_eq!(job.text, src);
    }
}
