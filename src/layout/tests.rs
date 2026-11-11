use super::{Piece, lines};
use crate::detect::Rect;

fn piece(x0: f32, y0: f32, x1: f32, y1: f32, text: &str) -> Piece {
    Piece {
        rect: Rect { x0, y0, x1, y1 },
        text: text.to_string(),
        score: 0.9,
    }
}

#[test]
fn pieces_on_one_line_join_left_to_right() {
    let found = lines(vec![
        piece(200.0, 12.0, 400.0, 40.0, "这是一段字。"),
        piece(10.0, 10.0, 190.0, 42.0, "第1行："),
        piece(10.0, 60.0, 300.0, 90.0, "Order #A-1024,"),
        piece(310.0, 62.0, 500.0, 88.0, "total $39.90"),
    ]);
    let texts: Vec<&str> = found.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(
        texts,
        ["第1行：这是一段字。", "Order #A-1024, total $39.90"]
    );
    assert_eq!(
        found[0].rect,
        Rect {
            x0: 10.0,
            y0: 10.0,
            x1: 400.0,
            y1: 42.0
        }
    );
}

#[test]
fn lines_go_top_to_bottom_even_when_found_out_of_order() {
    let found = lines(vec![
        piece(0.0, 100.0, 50.0, 130.0, "third"),
        piece(0.0, 0.0, 50.0, 30.0, "first"),
        piece(0.0, 50.0, 50.0, 80.0, "second"),
    ]);
    let texts: Vec<&str> = found.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, ["first", "second", "third"]);
}

#[test]
fn chinese_next_to_latin_gets_a_space() {
    let found = lines(vec![
        piece(0.0, 0.0, 40.0, 30.0, "价格"),
        piece(50.0, 0.0, 90.0, 30.0, "128"),
    ]);
    assert_eq!(found[0].text, "价格 128");
}
