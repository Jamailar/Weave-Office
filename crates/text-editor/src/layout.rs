//! Turning text into egui layout jobs: syntax colors for code, live-preview styling for
//! Markdown.

use crate::markdown::{Parsed, flag};
use crate::syntax::Token;
use egui::text::{LayoutJob, LayoutSection, TextFormat};
use egui::{Color32, FontFamily, FontId, Stroke};
use std::ops::Range;

pub const BOLD_FAMILY: &str = "weave-text-bold";
pub const MONO_FAMILY: &str = "weave-text-mono";

/// Colors for one appearance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub text: Color32,
    pub dim: Color32,
    pub comment: Color32,
    pub string: Color32,
    pub number: Color32,
    pub keyword: Color32,
    pub kind: Color32,
    pub function: Color32,
    pub key: Color32,
    pub tag: Color32,
    pub punctuation: Color32,
    pub meta: Color32,
    pub link: Color32,
    pub accent: Color32,
    pub highlight: Color32,
    pub code_bg: Color32,
    pub quote: Color32,
    pub match_bg: Color32,
    pub current_match_bg: Color32,
    pub line_bg: Color32,
}

impl Palette {
    pub fn new(dark: bool, text: Color32) -> Self {
        if dark {
            Self {
                text,
                dim: Color32::from_gray(120),
                comment: Color32::from_rgb(110, 125, 140),
                string: Color32::from_rgb(165, 214, 140),
                number: Color32::from_rgb(242, 170, 110),
                keyword: Color32::from_rgb(220, 140, 230),
                kind: Color32::from_rgb(110, 200, 220),
                function: Color32::from_rgb(120, 175, 250),
                key: Color32::from_rgb(130, 190, 255),
                tag: Color32::from_rgb(240, 120, 120),
                punctuation: Color32::from_gray(150),
                meta: Color32::from_rgb(230, 200, 110),
                link: Color32::from_rgb(130, 170, 255),
                accent: Color32::from_rgb(160, 130, 250),
                highlight: Color32::from_rgba_unmultiplied(230, 200, 60, 70),
                code_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                quote: Color32::from_gray(90),
                match_bg: Color32::from_rgba_unmultiplied(230, 190, 60, 60),
                current_match_bg: Color32::from_rgba_unmultiplied(250, 150, 40, 140),
                line_bg: Color32::from_rgba_unmultiplied(255, 255, 255, 8),
            }
        } else {
            Self {
                text,
                dim: Color32::from_gray(150),
                comment: Color32::from_rgb(110, 119, 129),
                string: Color32::from_rgb(10, 110, 50),
                number: Color32::from_rgb(190, 90, 0),
                keyword: Color32::from_rgb(170, 40, 160),
                kind: Color32::from_rgb(0, 120, 140),
                function: Color32::from_rgb(30, 90, 200),
                key: Color32::from_rgb(20, 80, 170),
                tag: Color32::from_rgb(190, 40, 40),
                punctuation: Color32::from_gray(110),
                meta: Color32::from_rgb(150, 110, 0),
                link: Color32::from_rgb(40, 90, 210),
                accent: Color32::from_rgb(120, 80, 230),
                highlight: Color32::from_rgba_unmultiplied(255, 220, 60, 110),
                code_bg: Color32::from_rgba_unmultiplied(0, 0, 0, 12),
                quote: Color32::from_gray(200),
                match_bg: Color32::from_rgba_unmultiplied(255, 210, 60, 110),
                current_match_bg: Color32::from_rgba_unmultiplied(255, 150, 30, 170),
                line_bg: Color32::from_rgba_unmultiplied(0, 0, 0, 7),
            }
        }
    }

    pub fn token(&self, token: Token) -> Color32 {
        match token {
            Token::Comment => self.comment,
            Token::String => self.string,
            Token::Number => self.number,
            Token::Keyword => self.keyword,
            Token::Type => self.kind,
            Token::Function => self.function,
            Token::Key => self.key,
            Token::Tag => self.tag,
            Token::Punctuation => self.punctuation,
            Token::Meta => self.meta,
        }
    }
}

/// What the layout needs to know besides the text.
#[derive(Clone, Debug)]
pub struct Style {
    pub palette: Palette,
    pub size: f32,
    pub mono: FontFamily,
    pub bold: FontFamily,
    pub proportional: FontFamily,
    pub wrap_width: f32,
}

impl Style {
    fn font(&self, size: f32, family: &FontFamily) -> FontId {
        FontId::new(size, family.clone())
    }
}

/// Lay out code: monospace, with syntax colors.
pub fn code_job(text: &str, tokens: &[(Range<usize>, Token)], style: &Style) -> LayoutJob {
    let base = TextFormat {
        font_id: style.font(style.size, &style.mono),
        color: style.palette.text,
        ..Default::default()
    };
    let mut job = LayoutJob {
        text: text.to_owned(),
        ..Default::default()
    };
    job.wrap.max_width = style.wrap_width;
    let mut at = 0;
    let push = |job: &mut LayoutJob, range: Range<usize>, color: Color32| {
        if range.is_empty() {
            return;
        }
        job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: egui::epaint::text::ByteIndex(range.start)
                ..egui::epaint::text::ByteIndex(range.end),
            format: TextFormat {
                color,
                ..base.clone()
            },
        });
    };
    for (range, token) in tokens {
        if range.start < at || range.end > text.len() {
            continue;
        }
        push(&mut job, at..range.start, style.palette.text);
        push(&mut job, range.clone(), style.palette.token(*token));
        at = range.end;
    }
    push(&mut job, at..text.len(), style.palette.text);
    if job.sections.is_empty() {
        job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: egui::epaint::text::ByteIndex(0)..egui::epaint::text::ByteIndex(0),
            format: base,
        });
    }
    job
}

/// How much of the Markdown syntax to show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reveal<'a> {
    /// Source mode: all syntax visible, no size changes.
    All,
    /// Live preview: syntax visible only on these lines (byte ranges of whole lines).
    Lines(&'a [Range<usize>]),
    /// Reading mode: no syntax anywhere.
    None,
}

pub fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.75,
        2 => 1.45,
        3 => 1.25,
        4 => 1.1,
        _ => 1.0,
    }
}

/// Lay out Markdown with live-preview styling.
pub fn markdown_job(text: &str, parsed: &Parsed, reveal: Reveal<'_>, style: &Style) -> LayoutJob {
    let palette = &style.palette;
    let mut job = LayoutJob {
        text: text.to_owned(),
        ..Default::default()
    };
    job.wrap.max_width = style.wrap_width;
    let source = reveal == Reveal::All;
    let mut tokens = parsed.tokens.iter().peekable();
    // A tiny, transparent font for hidden syntax.
    let hidden = TextFormat {
        font_id: FontId::new(0.5, style.proportional.clone()),
        color: Color32::TRANSPARENT,
        ..Default::default()
    };
    for (line_index, line) in parsed.lines.iter().enumerate() {
        let revealed = match reveal {
            Reveal::All => true,
            Reveal::None => false,
            Reveal::Lines(lines) => lines
                .iter()
                .any(|r| r.start <= line.range.end && line.range.start <= r.end),
        };
        let scale = if source {
            1.0
        } else {
            heading_scale(line.heading)
        };
        let size = style.size * scale;
        // The line's bytes plus its newline, so every byte belongs to a section.
        let end = parsed
            .lines
            .get(line_index + 1)
            .map_or(text.len(), |next| next.range.start);
        let mut at = line.range.start;
        while at < end {
            let flags = parsed.flags.get(at).copied().unwrap_or(0);
            while tokens.peek().is_some_and(|(range, _)| range.end <= at) {
                tokens.next();
            }
            let token = tokens
                .peek()
                .filter(|(range, _)| range.start <= at)
                .map(|(range, token)| (range.end, *token));
            let next_token_start = if token.is_none() {
                tokens.peek().map_or(usize::MAX, |(range, _)| range.start)
            } else {
                usize::MAX
            };
            let mut run_end = at + 1;
            while run_end < end
                && parsed.flags.get(run_end).copied().unwrap_or(0) == flags
                && token.is_none_or(|(token_end, _)| run_end < token_end)
                && (token.is_some() || run_end < next_token_start)
            {
                run_end += 1;
            }
            while !text.is_char_boundary(run_end) {
                run_end += 1;
            }
            let format = if flags & flag::MARKUP != 0 && !revealed {
                hidden.clone()
            } else {
                let mut format = TextFormat {
                    font_id: FontId::new(size, style.proportional.clone()),
                    color: palette.text,
                    ..Default::default()
                };
                if line.heading > 0 || flags & flag::BOLD != 0 {
                    format.font_id.family = style.bold.clone();
                }
                if line.heading == 6 && !source {
                    format.color = palette.dim;
                }
                if flags & (flag::MONO | flag::CODE) != 0 {
                    format.font_id = FontId::new(size * 0.92, style.mono.clone());
                }
                if flags & flag::CODE != 0 {
                    format.background = palette.code_bg;
                    format.color = palette.tag;
                }
                if flags & flag::ITALIC != 0 {
                    format.italics = true;
                }
                if flags & flag::STRIKE != 0 {
                    format.strikethrough = Stroke::new(1.0, palette.text);
                }
                if flags & flag::HIGHLIGHT != 0 {
                    format.background = palette.highlight;
                }
                if flags & flag::LINK != 0 {
                    format.color = palette.link;
                    if !source {
                        format.underline = Stroke::new(1.0, palette.link.gamma_multiply(0.5));
                    }
                }
                if flags & flag::IMAGE != 0 {
                    format.italics = true;
                }
                if flags & flag::TAG != 0 {
                    format.color = palette.accent;
                    format.background = palette.accent.gamma_multiply(0.12);
                }
                if flags & flag::QUOTE != 0 && flags & flag::CALLOUT == 0 {
                    format.color = palette.text.gamma_multiply(0.8);
                }
                if flags & flag::CALLOUT != 0 {
                    format.color = palette.accent;
                }
                if flags & flag::DONE != 0 && !source {
                    format.color = palette.dim;
                    format.strikethrough = Stroke::new(1.0, palette.dim);
                }
                if flags & (flag::MARKUP | flag::DIM) != 0 {
                    format.color = palette.dim;
                }
                if flags & flag::COVERED != 0 && !revealed {
                    format.color = Color32::TRANSPARENT;
                }
                if let Some((_, token)) = token {
                    format.color = palette.token(token);
                }
                if line.code && flags & flag::DIM == 0 && token.is_none() && flags & flag::MONO != 0
                {
                    format.color = palette.text;
                }
                format
            };
            job.sections.push(LayoutSection {
                leading_space: 0.0,
                byte_range: egui::epaint::text::ByteIndex(at)
                    ..egui::epaint::text::ByteIndex(run_end.min(end)),
                format,
            });
            at = run_end.min(end);
        }
    }
    if job.sections.is_empty() {
        job.sections.push(LayoutSection {
            leading_space: 0.0,
            byte_range: egui::epaint::text::ByteIndex(0)..egui::epaint::text::ByteIndex(0),
            format: TextFormat {
                font_id: FontId::new(style.size, style.proportional.clone()),
                color: palette.text,
                ..Default::default()
            },
        });
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse;

    fn style() -> Style {
        Style {
            palette: Palette::new(false, Color32::BLACK),
            size: 15.0,
            mono: FontFamily::Monospace,
            bold: FontFamily::Proportional,
            proportional: FontFamily::Proportional,
            wrap_width: 500.0,
        }
    }

    fn covers_text(job: &LayoutJob) {
        let mut at = 0;
        for section in &job.sections {
            assert_eq!(section.byte_range.start.0, at, "{:?}", job.sections);
            at = section.byte_range.end.0;
            assert!(job.text.is_char_boundary(at));
        }
        assert_eq!(at, job.text.len());
    }

    #[test]
    fn markdown_sections_cover_every_byte() {
        for text in [
            "",
            "# 标题\n\n正文 **粗体** 和 [链接](https://x)\n- [ ] 任务\n```rust\nfn main() {}\n```\n> 引用\n",
            "no newline at end *x*",
            "```\nunclosed",
            "---\na: b\n---\n",
        ] {
            let parsed = parse(text);
            for reveal in [Reveal::All, Reveal::None, Reveal::Lines(&[0..3])] {
                covers_text(&markdown_job(text, &parsed, reveal, &style()));
            }
        }
    }

    #[test]
    fn hidden_markup_only_away_from_the_cursor() {
        let text = "**b**\n**c**";
        let parsed = parse(text);
        let job = markdown_job(text, &parsed, Reveal::Lines(&[0..5]), &style());
        let color_at = |at: usize| {
            job.sections
                .iter()
                .find(|s| s.byte_range.start.0 <= at && at < s.byte_range.end.0)
                .unwrap()
                .format
                .color
        };
        assert_ne!(color_at(0), Color32::TRANSPARENT);
        assert_eq!(color_at(6), Color32::TRANSPARENT);
        assert_ne!(color_at(8), Color32::TRANSPARENT);
    }

    #[test]
    fn code_sections_cover_every_byte() {
        let text = "fn main() { \"中\" }";
        let mut tokens = Vec::new();
        crate::syntax::highlight(
            crate::syntax::Language::by_name("rust").unwrap(),
            text,
            0,
            &mut tokens,
        );
        covers_text(&code_job(text, &tokens, &style()));
        covers_text(&code_job("", &[], &style()));
    }
}
