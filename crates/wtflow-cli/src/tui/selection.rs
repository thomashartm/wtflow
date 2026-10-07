//! Select the rendered cells, including popups and details, without changing list focus.
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::{buffer::Buffer, prelude::*};

#[derive(Default)]
pub struct Selection {
    rendered: Buffer,
    start: Option<Position>,
    end: Position,
    dragging: bool,
    pub text: Option<String>,
}

impl Selection {
    pub fn clear(&mut self) {
        self.start = None;
        self.dragging = false;
        self.text = None;
    }

    /// Returns true for selection events; a plain click is dispatched on release.
    pub fn mouse(&mut self, mouse: MouseEvent) -> bool {
        let position = Position::new(mouse.column, mouse.row);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.clear();
                if self.rendered.area.contains(position) {
                    self.start = Some(position);
                    self.end = position;
                    self.dragging = true;
                }
                true
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                self.end = self.clamp(position);
                true
            }
            MouseEventKind::Up(MouseButton::Left) if self.dragging => {
                self.end = self.clamp(position);
                self.dragging = false;
                if self.start == Some(self.end) {
                    self.clear();
                    return false;
                }
                self.text = Some(self.selected_text());
                true
            }
            MouseEventKind::Moved if self.dragging => true,
            _ => {
                self.clear();
                false
            }
        }
    }

    fn clamp(&self, position: Position) -> Position {
        let area = self.rendered.area;
        Position::new(
            position.x.clamp(area.x, area.right().saturating_sub(1)),
            position.y.clamp(area.y, area.bottom().saturating_sub(1)),
        )
    }

    fn range(&self) -> Option<(Position, Position)> {
        let start = self.start?;
        Some(if (start.y, start.x) <= (self.end.y, self.end.x) {
            (start, self.end)
        } else {
            (self.end, start)
        })
    }

    fn selected_text(&self) -> String {
        let Some((start, end)) = self.range() else {
            return String::new();
        };
        let mut lines = Vec::new();
        for y in start.y..=end.y {
            let left = if y == start.y {
                start.x
            } else {
                self.rendered.area.x
            };
            let right = if y == end.y {
                end.x + 1
            } else {
                self.rendered.area.right()
            };
            let mut line = String::new();
            let mut x = self.rendered.area.x;
            while x < right {
                let Some(cell) = self.rendered.cell((x, y)) else {
                    break;
                };
                let width = Span::raw(cell.symbol()).width().max(1) as u16;
                if x + width > left {
                    line.push_str(cell.symbol());
                }
                x += width;
            }
            lines.push(line.trim_end().to_owned());
        }
        lines.join("\n")
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let buffer = frame.buffer_mut();
        if self.rendered.area != buffer.area {
            self.clear();
        }
        if self.dragging {
            // Live job output must not move text out from under a drag.
            *buffer = self.rendered.clone();
        } else if self.start.is_none() {
            self.rendered = buffer.clone();
        }
        if let Some((start, end)) = self.range() {
            for y in start.y..=end.y {
                let left = if y == start.y { start.x } else { buffer.area.x };
                let right = if y == end.y {
                    end.x + 1
                } else {
                    buffer.area.right()
                };
                buffer.set_style(
                    Rect::new(left, y, right - left, 1),
                    Style::default().bg(Color::Cyan).fg(Color::Black),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    fn mouse(kind: MouseEventKind, x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }
    }
    #[test]
    fn drag_copies_unicode_cells_in_either_direction_and_click_is_not_copy() {
        for reverse in [false, true] {
            let mut selection = Selection {
                rendered: Buffer::with_lines(["café 猫🐈", "next line"]),
                ..Selection::default()
            };
            let (start, end) = if reverse {
                ((3, 1), (5, 0))
            } else {
                ((5, 0), (3, 1))
            };
            assert!(selection.mouse(mouse(
                MouseEventKind::Down(MouseButton::Left),
                start.0,
                start.1
            )));
            assert!(selection.mouse(mouse(MouseEventKind::Drag(MouseButton::Left), end.0, end.1)));
            assert!(selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), end.0, end.1)));
            assert_eq!(selection.text.as_deref(), Some("猫🐈\nnext"));
            selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
            assert!(!selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 0, 0)));
            assert!(selection.text.is_none());
        }
    }
    #[test]
    fn drag_clamps_to_screen_and_resize_clears_selection() {
        let mut selection = Selection {
            rendered: Buffer::with_lines(["hello"]),
            ..Selection::default()
        };
        selection.mouse(mouse(MouseEventKind::Down(MouseButton::Left), 1, 0));
        selection.mouse(mouse(MouseEventKind::Up(MouseButton::Left), 200, 100));
        assert_eq!(selection.text.as_deref(), Some("ello"));
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(10, 2)).unwrap();
        terminal.draw(|frame| selection.draw(frame)).unwrap();
        assert!(selection.text.is_none());
    }
}
