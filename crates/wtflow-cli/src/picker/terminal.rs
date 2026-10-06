use super::Item;
use anyhow::Result;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyModifiers,
    },
    execute, queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal::{
        self, Clear, ClearType, DisableLineWrap, EnableLineWrap, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use std::{
    io::{self, Write},
    path::Path,
};

/// Restore the shell even on an I/O error, Escape, Ctrl-C, or unwinding.
struct Screen;
impl Screen {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let screen = Self;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            Hide,
            DisableLineWrap,
            EnableBracketedPaste
        )?;
        Ok(screen)
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            ResetColor,
            SetAttribute(Attribute::Reset),
            DisableBracketedPaste,
            EnableLineWrap,
            Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

struct Selection {
    query: String,
    tests: bool,
    selected: usize,
    offset: usize,
    allow_new: bool,
}
impl Selection {
    fn new(items: &[Item], allow_new: bool, initial_query: &str) -> Self {
        Self {
            query: initial_query.to_owned(),
            tests: false,
            selected: usize::from(allow_new && items.iter().any(|i| !i.is_test)),
            offset: 0,
            allow_new,
        }
    }
    fn matches(&self, items: &[Item]) -> Vec<usize> {
        let filter = crate::filter::Filter::new(&self.query);
        let mut matches = Vec::new();
        if self.allow_new {
            matches.push(items.len());
        }
        matches.extend(
            items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    (self.tests || !item.is_test)
                        && filter.matches(&[&item.title, &item.symbol, &item.path])
                })
                .map(|(i, _)| i),
        );
        matches
    }
    fn reset(&mut self, items: &[Item]) {
        self.selected = usize::from(self.allow_new && self.matches(items).len() > 1);
        self.offset = 0;
    }
    fn viewport(&mut self, count: usize, rows: usize) {
        self.selected = self.selected.min(count.saturating_sub(1));
        self.offset = self.offset.min(self.selected);
        if self.selected >= self.offset + rows {
            self.offset = self.selected + 1 - rows;
        }
    }
    fn move_by(&mut self, delta: isize, count: usize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
    }
}

fn text_at(out: &mut impl Write, row: u16, text: &str, width: u16) -> io::Result<()> {
    // Never interpret source-derived terminal control characters as commands.
    let text: String = text
        .chars()
        .filter(|c| !c.is_control())
        .take(width.saturating_sub(1) as usize)
        .collect();
    queue!(out, MoveTo(0, row), Print(text))
}

fn draw(
    root: &Path,
    title: &str,
    items: &[Item],
    state: &mut Selection,
    matches: &[usize],
) -> Result<usize> {
    let (width, height) = terminal::size()?;
    let mut out = io::stdout().lock();
    queue!(out, MoveTo(0, 0), Clear(ClearType::All))?;
    if height < 12 || width < 40 {
        text_at(
            &mut out,
            0,
            "Enlarge terminal (40 columns x 12 rows).",
            width,
        )?;
        if height > 1 {
            text_at(&mut out, 1, "Esc or Ctrl-C to quit.", width)?;
        }
        out.flush()?;
        return Ok(0);
    }
    let rows = ((height - 10) / 2).max(1) as usize;
    state.viewport(matches.len(), rows);
    queue!(
        out,
        SetAttribute(Attribute::Bold),
        SetForegroundColor(Color::Cyan)
    )?;
    text_at(&mut out, 0, &format!("wtflow / {title}"), width)?;
    queue!(out, ResetColor, SetAttribute(Attribute::Reset))?;
    text_at(
        &mut out,
        1,
        &format!(
            "Project: {}",
            root.file_name()
                .unwrap_or(root.as_os_str())
                .to_string_lossy()
        ),
        width,
    )?;
    text_at(&mut out, 2, &root.display().to_string(), width)?;
    text_at(&mut out, 3, &format!("Search: {}_", state.query), width)?;
    let count = matches.len() - usize::from(state.allow_new);
    text_at(
        &mut out,
        4,
        &format!(
            "{count} matches | Tests: {}",
            if state.tests { "shown" } else { "hidden" }
        ),
        width,
    )?;
    for (row, position) in (state.offset..matches.len()).take(rows).enumerate() {
        let index = matches[position];
        let chosen = position == state.selected;
        if chosen {
            queue!(
                out,
                SetBackgroundColor(Color::DarkCyan),
                SetForegroundColor(Color::White),
                SetAttribute(Attribute::Bold)
            )?;
        }
        let marker = if chosen { ">" } else { " " };
        let title = if index == items.len() {
            "+ Analyze a new flow".to_owned()
        } else {
            format!(
                "{}{}",
                items[index].title,
                if items[index].is_test { " [test]" } else { "" }
            )
        };
        text_at(
            &mut out,
            5 + row as u16 * 2,
            &format!(" {marker} {title}"),
            width,
        )?;
        queue!(
            out,
            ResetColor,
            SetAttribute(Attribute::Reset),
            SetForegroundColor(Color::DarkGrey)
        )?;
        let subtitle = if index == items.len() {
            "Find a starting point in this project"
        } else {
            &items[index].symbol
        };
        text_at(
            &mut out,
            6 + row as u16 * 2,
            &format!("   {subtitle}"),
            width,
        )?;
        queue!(out, ResetColor)?;
    }
    if count == 0 && !state.allow_new {
        text_at(
            &mut out,
            5,
            "No application entrypoints match in this project.",
            width,
        )?;
        text_at(
            &mut out,
            6,
            "Change search, F2 for tests, or Esc to leave.",
            width,
        )?;
    }
    let path = matches
        .get(state.selected)
        .and_then(|i| items.get(*i))
        .map(|i| i.path.as_str())
        .unwrap_or("");
    text_at(&mut out, height - 4, path, width)?;
    text_at(
        &mut out,
        height - 3,
        "Up/Down move | Enter select | Type to filter (* and ?)",
        width,
    )?;
    text_at(
        &mut out,
        height - 2,
        "PgUp/PgDn scroll | F2 tests | Esc clear/back | Ctrl-C quit",
        width,
    )?;
    text_at(
        &mut out,
        height - 1,
        &format!(
            "{} / {}",
            if matches.is_empty() {
                0
            } else {
                state.selected + 1
            },
            matches.len()
        ),
        width,
    )?;
    out.flush()?;
    Ok(rows)
}

pub fn choose(
    root: &Path,
    title: &str,
    items: &[Item],
    allow_new: bool,
    initial_query: &str,
) -> Result<Option<usize>> {
    let _screen = Screen::enter()?;
    let mut state = Selection::new(items, allow_new, initial_query);
    loop {
        let matches = state.matches(items);
        let rows = draw(root, title, items, &mut state, &matches)?;
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                let control = key.modifiers.contains(KeyModifiers::CONTROL);
                match key.code {
                    KeyCode::Char('c') if control => return Ok(None),
                    KeyCode::Esc if state.query.is_empty() => return Ok(None),
                    KeyCode::Esc | KeyCode::Char('u') if control || key.code == KeyCode::Esc => {
                        state.query.clear();
                        state.reset(items);
                    }
                    _ if rows == 0 => {}
                    KeyCode::Up => state.move_by(-1, matches.len()),
                    KeyCode::Down => state.move_by(1, matches.len()),
                    KeyCode::PageUp => state.move_by(-(rows as isize), matches.len()),
                    KeyCode::PageDown => state.move_by(rows as isize, matches.len()),
                    KeyCode::Home => state.selected = 0,
                    KeyCode::End => state.selected = matches.len().saturating_sub(1),
                    KeyCode::Enter => {
                        if let Some(index) = matches.get(state.selected) {
                            return Ok(Some(*index));
                        }
                    }
                    KeyCode::F(2) => {
                        state.tests = !state.tests;
                        state.reset(items);
                    }
                    KeyCode::Char('n') if control && allow_new => return Ok(Some(items.len())),
                    KeyCode::Backspace => {
                        state.query.pop();
                        state.reset(items);
                    }
                    KeyCode::Char(c) if !control && !key.modifiers.contains(KeyModifiers::ALT) => {
                        state.query.push(c);
                        state.reset(items);
                    }
                    _ => {}
                }
            }
            Event::Paste(text) => {
                state.query.extend(text.chars().filter(|c| !c.is_control()));
                state.reset(items);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_stays_visible_and_filtering_keeps_identity() {
        let items: Vec<_> = (0..20)
            .map(|i| Item {
                title: format!("Order {i}"),
                symbol: format!("Controller.m{i}"),
                path: format!("src/{i}.ts"),
                is_test: i == 19,
            })
            .collect();
        let mut state = Selection::new(&items, true, "");
        let matches = state.matches(&items);
        assert_eq!(matches[state.selected], 0);
        state.move_by(12, matches.len());
        state.viewport(matches.len(), 5);
        assert_eq!(state.offset, 9);
        assert_eq!(matches[state.selected], 12);
        state.query = "ORDER m17".into();
        state.reset(&items);
        let matches = state.matches(&items);
        assert_eq!(matches[state.selected], 17);
        state.move_by(-1, matches.len());
        assert_eq!(matches[state.selected], items.len());
        state.query = "m19".into();
        state.reset(&items);
        assert_eq!(state.matches(&items), vec![items.len()]);
        state.tests = true;
        state.reset(&items);
        assert_eq!(state.matches(&items)[state.selected], 19);
        state.viewport(0, 1);
        assert_eq!(state.selected, 0);
    }
}
