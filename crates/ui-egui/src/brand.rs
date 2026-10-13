//! Original MIXEL monogram, drawn in code under MIT OR Apache-2.0.

use egui::{Color32, Rect, Stroke, pos2, vec2};

/// Draw the suite's original M, `height` points square, vertically centred at `left_center`.
/// Returns the drawn rectangle so neighbouring text can be placed beside it.
pub fn paint_mark(ui: &egui::Ui, left_center: egui::Pos2, height: f32) -> Rect {
    let area = Rect::from_min_size(pos2(left_center.x, left_center.y - height / 2.0), vec2(height, height));
    let point = |x: f32, y: f32| pos2(area.left() + height * x, area.top() + height * y);
    let vertices = [point(0.18, 0.82), point(0.18, 0.18), point(0.5, 0.53), point(0.82, 0.18), point(0.82, 0.82)];
    let stroke = Stroke::new(height * 0.11, Color32::from_rgb(0x21, 0xb6, 0xa8));
    for pair in vertices.windows(2) {
        ui.painter().line_segment([pair[0], pair[1]], stroke);
    }
    area
}
