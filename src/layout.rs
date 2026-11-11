//! 把认出来的一段段排成一行行：上下有一半以上重叠的算同一行，行照上沿从上到下，行里照左沿从左到右。同一行的几段之间加
//! 一个空格，两边都是中日文的不加（找字的模型有时把一行从标点处切开）。

use crate::detect::Rect;

/// 认出来的一段：框和字。
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub rect: Rect,
    pub text: String,
    pub score: f32,
}

/// 排好的一行。
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    /// 这一行几段合起来的框。
    pub rect: Rect,
    /// 几段分数里最低的。
    pub score: f32,
}

/// 排成行。
#[must_use]
pub fn lines(mut pieces: Vec<Piece>) -> Vec<Line> {
    pieces.sort_by(|a, b| a.rect.y0.total_cmp(&b.rect.y0));
    let mut rows: Vec<(Rect, Vec<Piece>)> = Vec::new();
    for piece in pieces {
        let height = piece.rect.height();
        let joined = rows.iter_mut().find(|(span, _)| {
            let overlap = piece.rect.y1.min(span.y1) - piece.rect.y0.max(span.y0);
            overlap > 0.5 * height.min(span.height())
        });
        match joined {
            Some((span, members)) => {
                *span = union(*span, piece.rect);
                members.push(piece);
            }
            None => rows.push((piece.rect, vec![piece])),
        }
    }
    rows.sort_by(|a, b| a.0.y0.total_cmp(&b.0.y0));
    rows.into_iter()
        .map(|(rect, mut members)| {
            members.sort_by(|a, b| a.rect.x0.total_cmp(&b.rect.x0));
            let score = members.iter().map(|piece| piece.score).fold(1.0, f32::min);
            let mut text = String::new();
            for piece in &members {
                if needs_space(&text, &piece.text) {
                    text.push(' ');
                }
                text.push_str(&piece.text);
            }
            Line { text, rect, score }
        })
        .collect()
}

fn union(a: Rect, b: Rect) -> Rect {
    Rect {
        x0: a.x0.min(b.x0),
        y0: a.y0.min(b.y0),
        x1: a.x1.max(b.x1),
        y1: a.y1.max(b.y1),
    }
}

/// 前面已有的字和接上来的一段之间要不要空格。
fn needs_space(before: &str, after: &str) -> bool {
    match (before.chars().last(), after.chars().next()) {
        (Some(last), Some(first)) => !(wide(last) && wide(first)),
        _ => false,
    }
}

/// 中日韩的字和全角标点：前后不加空格。
fn wide(character: char) -> bool {
    matches!(u32::from(character),
        0x1100..=0x11FF
        | 0x2E80..=0x303F
        | 0x3040..=0x30FF
        | 0x3130..=0x318F
        | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF
        | 0xAC00..=0xD7AF
        | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F
        | 0xFF00..=0xFFEF
        | 0x20000..=0x2FA1F)
}

#[cfg(test)]
mod tests;
