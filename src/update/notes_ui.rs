//! Lightweight release-notes view for the update modal (egui-safe glyphs + links).
//!
//! Color literals mirror `ui::icons` chrome (avoid `update` ↔ `ui` module cycle).

use eframe::egui::{self, Color32, RichText};

const CHROME_TEXT: Color32 = Color32::from_rgb(230, 232, 238);
const CHROME_MUTED: Color32 = Color32::from_rgb(175, 182, 196);
const CHROME_ACCENT_BORDER: Color32 = Color32::from_rgb(100, 160, 220);

/// Draw truncated GitHub release body with headings, bullets, and clickable links.
///
/// Avoids Unicode arrows (egui default fonts lack them) and renders
/// `[label](url)` as hyperlinks with a clean display name.
pub fn draw_release_notes(ui: &mut egui::Ui, body: &str) {
    let notes = crate::update::truncate_notes(body);
    if notes == "(No release notes.)" {
        ui.label(RichText::new(notes).color(CHROME_MUTED));
        return;
    }

    let mut in_table = false;
    for raw_line in notes.lines() {
        let line = raw_line.trim_end();
        let trimmed = line.trim();

        // Skip markdown tables (pipe rows) — too dense for the modal.
        if trimmed.starts_with('|') {
            if !in_table {
                in_table = true;
                ui.label(
                    RichText::new("(See release page for download table.)")
                        .italics()
                        .color(CHROME_MUTED),
                );
            }
            continue;
        }
        in_table = false;

        if trimmed.is_empty() {
            ui.add_space(4.0);
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("### ") {
            ui.label(
                RichText::new(sanitize_display(rest.trim()))
                    .strong()
                    .color(CHROME_TEXT),
            );
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("## ") {
            ui.add_space(2.0);
            ui.label(
                RichText::new(sanitize_display(rest.trim()))
                    .strong()
                    .size(14.0)
                    .color(CHROME_TEXT),
            );
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("# ") {
            ui.label(
                RichText::new(sanitize_display(rest.trim()))
                    .strong()
                    .size(15.0)
                    .color(CHROME_TEXT),
            );
            continue;
        }

        let bullet = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "));
        if let Some(rest) = bullet {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new("•").color(CHROME_MUTED));
                draw_inline_markdown(ui, rest, false);
            });
            continue;
        }

        ui.horizontal_wrapped(|ui| {
            draw_inline_markdown(ui, trimmed, false);
        });
    }
}

/// Replace glyphs missing from egui's default font (e.g. → shows as a box).
fn sanitize_display(s: &str) -> String {
    s.replace('→', "->")
        .replace('←', "<-")
        .replace('—', "-")
        .replace('–', "-")
}

/// Render one line of inline markdown: `**bold**` and `[label](url)`.
/// Recurses into bold so `**[Learn…](url)**` stays a clickable link.
fn draw_inline_markdown(ui: &mut egui::Ui, line: &str, strong: bool) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Markdown link: [label](url)
        if chars[i] == '[' {
            if let Some((label, url, end)) = parse_md_link(&chars, i) {
                let label = sanitize_display(&label);
                let mut text = RichText::new(label).color(CHROME_ACCENT_BORDER);
                if strong {
                    text = text.strong();
                }
                ui.hyperlink_to(text, url);
                i = end;
                continue;
            }
        }
        // Bold: **text** (may wrap a link — recurse so the link stays clickable)
        if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
            if let Some((inner, end)) = parse_bold(&chars, i) {
                draw_inline_markdown(ui, &inner, true);
                i = end;
                continue;
            }
        }

        // Plain run until next special marker.
        let start = i;
        while i < chars.len() {
            if chars[i] == '[' {
                break;
            }
            if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '*' {
                break;
            }
            i += 1;
        }
        if i > start {
            let plain: String = chars[start..i].iter().collect();
            let mut text = RichText::new(sanitize_display(&plain)).color(if strong {
                CHROME_TEXT
            } else {
                CHROME_MUTED
            });
            if strong {
                text = text.strong();
            }
            ui.label(text);
        } else {
            // Unparsed special — emit as-is so we don't spin.
            ui.label(RichText::new(chars[i].to_string()).color(CHROME_MUTED));
            i += 1;
        }
    }
}

fn parse_md_link(chars: &[char], start: usize) -> Option<(String, String, usize)> {
    // start at '['
    let mut j = start + 1;
    while j < chars.len() && chars[j] != ']' {
        j += 1;
    }
    if j >= chars.len() || j + 1 >= chars.len() || chars[j + 1] != '(' {
        return None;
    }
    let label: String = chars[start + 1..j].iter().collect();
    let mut k = j + 2;
    while k < chars.len() && chars[k] != ')' {
        k += 1;
    }
    if k >= chars.len() {
        return None;
    }
    let url: String = chars[j + 2..k].iter().collect();
    if label.is_empty() || url.is_empty() {
        return None;
    }
    Some((label, url, k + 1))
}

fn parse_bold(chars: &[char], start: usize) -> Option<(String, usize)> {
    // start at first '*' of '**'
    let mut j = start + 2;
    while j + 1 < chars.len() {
        if chars[j] == '*' && chars[j + 1] == '*' {
            let inner: String = chars[start + 2..j].iter().collect();
            if inner.is_empty() {
                return None;
            }
            return Some((inner, j + 2));
        }
        j += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_arrow() {
        assert_eq!(sanitize_display("Pro → site"), "Pro -> site");
    }

    #[test]
    fn parse_link_and_bold() {
        let chars: Vec<char> = "**[Learn about TopicBench Pro →](https://example.com)**"
            .chars()
            .collect();
        // Outer bold wraps the link in release notes; parser sees bold first when used that way.
        let (inner, end) = parse_bold(&chars, 0).expect("bold");
        assert_eq!(inner, "[Learn about TopicBench Pro →](https://example.com)");
        assert_eq!(end, chars.len());

        let link_chars: Vec<char> = inner.chars().collect();
        let (label, url, _) = parse_md_link(&link_chars, 0).expect("link");
        assert_eq!(label, "Learn about TopicBench Pro →");
        assert_eq!(url, "https://example.com");
        assert_eq!(sanitize_display(&label), "Learn about TopicBench Pro ->");
    }
}
