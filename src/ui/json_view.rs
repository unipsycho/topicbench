//! Pretty-print + light syntax highlight for sticky Value / tree JSON payloads.

use eframe::egui::{self, Align, Color32, FontId, Layout, TextFormat};
use eframe::epaint::text::{LayoutJob, LayoutSection};
use std::hash::Hash;

/// Keys / structure punctuation on dark chrome.
const C_KEY: Color32 = Color32::from_rgb(140, 190, 255);
const C_STRING: Color32 = Color32::from_rgb(180, 220, 150);
const C_NUMBER: Color32 = Color32::from_rgb(230, 190, 120);
const C_BOOL: Color32 = Color32::from_rgb(120, 200, 230);
const C_NULL: Color32 = Color32::from_rgb(150, 150, 160);
const C_PUNCT: Color32 = Color32::from_rgb(170, 176, 188);
const C_DEFAULT: Color32 = Color32::from_rgb(220, 222, 228);
/// Topic-name gray (matches ribbon caption / weak chrome).
pub const C_TOPIC: Color32 = Color32::from_rgb(170, 176, 188);

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

/// One-line preview for dense tree rows (newlines → spaces, truncated at char bounds).
pub fn preview_line(payload: &str, max_chars: usize) -> String {
    let src = try_pretty_json(payload).unwrap_or_else(|| payload.to_string());
    let flat: String = src
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    truncate_chars(&flat, max_chars)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    // ASCII "..." only — never mid-codepoint (Label::truncate + LayoutJob panics).
    let trunc: String = s.chars().take(max_chars.saturating_sub(3)).collect();
    format!("{trunc}...")
}

/// `topic = {…}` LayoutJob: gray topic name, syntax colors on the payload side.
pub fn row_preview_job(label: &str, payload: &str, max_json_chars: usize, font_size: f32) -> LayoutJob {
    let font = FontId::monospace(font_size);
    let json = preview_line(payload, max_json_chars);
    let prefix = format!("{label} = ");
    let text = format!("{prefix}{json}");
    let mut job = LayoutJob {
        text: text.clone(),
        break_on_newline: false,
        halign: Align::LEFT,
        ..Default::default()
    };
    // Topic segment in chrome gray so highlighted payload stands out.
    push(&mut job, 0, prefix.len(), C_TOPIC, font.clone());
    highlight_range(&mut job, &text, prefix.len(), text.len(), &font);
    job
}

/// Payload-only LayoutJob (syntax highlight) for tree rows when copy sits at `topic =`.
pub fn payload_preview_job(payload: &str, max_chars: usize, font_size: f32) -> LayoutJob {
    let font = FontId::monospace(font_size);
    let text = preview_line(payload, max_chars);
    let mut job = LayoutJob {
        text: text.clone(),
        break_on_newline: false,
        halign: Align::LEFT,
        ..Default::default()
    };
    highlight_range(&mut job, &text, 0, text.len(), &font);
    job
}

/// Draw pretty JSON left-aligned with syntax colors. `id_salt` must be unique per row.
/// `max_height`: `None` fills available height (Payload panel); sticky cards pass a cap.
pub fn draw_pretty(ui: &mut egui::Ui, pretty: &str, id_salt: impl Hash, max_height: Option<f32>) {
    let font_size = ui.text_style_height(&egui::TextStyle::Monospace);
    let height = max_height.unwrap_or_else(|| ui.available_height().max(80.0));
    ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
        ui.label(egui::RichText::new("JSON").small().weak());
        egui::ScrollArea::vertical()
            .id_salt(id_salt)
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                    let job = highlight_job(pretty, font_size);
                    ui.add(
                        egui::Label::new(job)
                            .wrap()
                            .selectable(true)
                            .halign(Align::LEFT),
                    );
                });
            });
    });
}

/// Hover UI with its own ScrollArea (sticky/tooltips that aren't inside another SA).
#[allow(dead_code)]
pub fn hover_payload(ui: &mut egui::Ui, payload: &str, id_salt: impl Hash) {
    if let Some(pretty) = try_pretty_json(payload) {
        draw_pretty(ui, &pretty, id_salt, Some(220.0));
    } else {
        hover_payload_body(ui, payload);
    }
}

/// Payload body for an outer ScrollArea (no nested scroll — used by tree hover popup).
pub fn hover_payload_body(ui: &mut egui::Ui, payload: &str) {
    if let Some(pretty) = try_pretty_json(payload) {
        let font_size = ui.text_style_height(&egui::TextStyle::Monospace);
        ui.label(egui::RichText::new("JSON").small().weak());
        ui.set_max_width(ui.available_width());
        let job = highlight_job(&pretty, font_size);
        ui.add(
            egui::Label::new(job)
                .wrap()
                .selectable(true)
                .halign(Align::LEFT),
        );
    } else {
        ui.label(egui::RichText::new(payload).monospace());
    }
}

/// Tokenize pretty JSON into a colored left-aligned `LayoutJob`.
fn highlight_job(src: &str, font_size: f32) -> LayoutJob {
    let font = FontId::monospace(font_size);
    let mut job = LayoutJob {
        text: src.to_string(),
        break_on_newline: true,
        halign: Align::LEFT,
        ..Default::default()
    };
    highlight_range(&mut job, src, 0, src.len(), &font);
    job
}

/// Highlight `text[from..to]` (byte range) into an existing job (JSON tokenizer).
fn highlight_range(job: &mut LayoutJob, text: &str, from: usize, to: usize, font: &FontId) {
    let bytes = text.as_bytes();
    let to = to.min(bytes.len());
    let mut i = from.min(to);
    while i < to {
        let start = i;
        let b = bytes[i];

        if b.is_ascii_whitespace() {
            while i < to && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            push(job, start, i, C_DEFAULT, font.clone());
            continue;
        }

        // String (key if followed by ':')
        if b == b'"' {
            i += 1;
            while i < to {
                if bytes[i] == b'\\' && i + 1 < to {
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
            while j < to && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let color = if j < to && bytes[j] == b':' {
                C_KEY
            } else {
                C_STRING
            };
            push(job, start, i, color, font.clone());
            continue;
        }

        if matches!(b, b'{' | b'}' | b'[' | b']' | b':' | b',') {
            i += 1;
            push(job, start, i, C_PUNCT, font.clone());
            continue;
        }

        if b == b'-' || b.is_ascii_digit() {
            i += 1;
            while i < to
                && (bytes[i].is_ascii_digit()
                    || matches!(bytes[i], b'.' | b'e' | b'E' | b'+' | b'-'))
            {
                i += 1;
            }
            push(job, start, i, C_NUMBER, font.clone());
            continue;
        }

        if b.is_ascii_alphabetic() {
            while i < to && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let word = &text[start..i];
            let color = match word {
                "true" | "false" => C_BOOL,
                "null" => C_NULL,
                _ => C_DEFAULT,
            };
            push(job, start, i, color, font.clone());
            continue;
        }

        i += 1;
        push(job, start, i, C_DEFAULT, font.clone());
    }
}

fn push(job: &mut LayoutJob, start: usize, end: usize, color: Color32, font: FontId) {
    push_bg(job, start, end, color, font, Color32::TRANSPARENT);
}

fn push_bg(
    job: &mut LayoutJob,
    start: usize,
    end: usize,
    color: Color32,
    font: FontId,
    background: Color32,
) {
    if start >= end {
        return;
    }
    job.sections.push(LayoutSection {
        leading_space: 0.0,
        byte_range: start..end,
        format: TextFormat {
            font_id: font,
            color,
            background,
            valign: Align::Center,
            ..Default::default()
        },
    });
}

/// Soft / strong search span backgrounds (Payload find).
const SEARCH_HIT_BG: Color32 = Color32::from_rgb(70, 65, 35);
const SEARCH_CURRENT_BG: Color32 = Color32::from_rgb(120, 100, 40);

/// One pretty-JSON / Value line with optional search-span backgrounds.
/// `hits` and `current` are byte ranges relative to `line`.
pub fn line_job_with_search(
    line: &str,
    font_size: f32,
    hits: &[(usize, usize)],
    current: Option<(usize, usize)>,
) -> LayoutJob {
    let font = FontId::monospace(font_size);
    let mut base = LayoutJob {
        text: line.to_string(),
        break_on_newline: false,
        halign: Align::LEFT,
        ..Default::default()
    };
    highlight_range(&mut base, line, 0, line.len(), &font);
    apply_search_backgrounds(&mut base, line.len(), hits, current);
    base
}

/// Plain monospace line with search-span backgrounds (non-JSON Value).
pub fn plain_line_job_with_search(
    line: &str,
    font_size: f32,
    hits: &[(usize, usize)],
    current: Option<(usize, usize)>,
) -> LayoutJob {
    let font = FontId::monospace(font_size);
    let mut job = LayoutJob {
        text: line.to_string(),
        break_on_newline: false,
        halign: Align::LEFT,
        ..Default::default()
    };
    push(&mut job, 0, line.len(), C_DEFAULT, font);
    apply_search_backgrounds(&mut job, line.len(), hits, current);
    job
}

/// Split existing colored sections so match spans get a background.
fn apply_search_backgrounds(
    job: &mut LayoutJob,
    len: usize,
    hits: &[(usize, usize)],
    current: Option<(usize, usize)>,
) {
    if hits.is_empty() || job.sections.is_empty() {
        return;
    }
    let old = std::mem::take(&mut job.sections);
    for sec in old {
        let cursor = sec.byte_range.start;
        let end = sec.byte_range.end.min(len);
        // Collect cut points inside this section from hit edges.
        let mut cuts = vec![cursor, end];
        for &(hs, he) in hits {
            if he <= cursor || hs >= end {
                continue;
            }
            cuts.push(hs.max(cursor));
            cuts.push(he.min(end));
        }
        cuts.sort_unstable();
        cuts.dedup();
        for w in cuts.windows(2) {
            let a = w[0];
            let b = w[1];
            if a >= b {
                continue;
            }
            let mut fmt = sec.format.clone();
            if current.is_some_and(|(cs, ce)| a < ce && b > cs) {
                fmt.background = SEARCH_CURRENT_BG;
            } else if hits.iter().any(|&(hs, he)| a < he && b > hs) {
                fmt.background = SEARCH_HIT_BG;
            }
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: a..b,
                format: fmt,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_object_indented() {
        let p = try_pretty_json(r#"{"a":{"b":1},"c":true}"#).unwrap();
        assert!(p.contains('\n'));
        assert!(p.contains("  \"a\""));
        assert!(p.contains("    \"b\""));
    }

    #[test]
    fn reject_plain_text() {
        assert!(try_pretty_json("hello").is_none());
        assert!(try_pretty_json("42").is_none());
    }

    #[test]
    fn highlight_covers_all_bytes() {
        let src = "{\n  \"ok\": true,\n  \"n\": 2.5\n}";
        let job = highlight_job(src, 12.0);
        let covered: usize = job.sections.iter().map(|s| s.byte_range.len()).sum();
        assert_eq!(covered, src.len());
        assert_eq!(job.halign, Align::LEFT);
    }

    #[test]
    fn preview_flattens() {
        let p = preview_line(r#"{"a":1}"#, 80);
        assert!(!p.contains('\n'));
        assert!(p.contains("\"a\""));
    }

    #[test]
    fn row_preview_job_covers_bytes() {
        let job = row_preview_job("definition", r#"{"a":1,"b":"x"}"#, 80, 12.0);
        let covered: usize = job.sections.iter().map(|s| s.byte_range.len()).sum();
        assert_eq!(covered, job.text.len());
        assert!(job.text.starts_with("definition = "));
        assert_eq!(job.sections[0].format.color, C_TOPIC);
    }

    #[test]
    fn payload_preview_job_covers_bytes() {
        let job = payload_preview_job(r#"{"a":1}"#, 80, 12.0);
        let covered: usize = job.sections.iter().map(|s| s.byte_range.len()).sum();
        assert_eq!(covered, job.text.len());
        assert!(!job.text.contains('='));
    }
}
