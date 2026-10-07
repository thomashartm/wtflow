//! Form controls and hit targets shared by keyboard and mouse interaction.
use super::{catalog, clean, panel, popup};
use ratatui::{
    prelude::*,
    widgets::{Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Field(usize),
    Language(usize, &'static str),
    Submit,
    Cancel,
}

pub fn controls(form: &catalog::Form) -> Vec<Control> {
    let mut controls = Vec::new();
    for (i, field) in form.fields.iter().enumerate() {
        if form.name == "index" && field.id == "lang" {
            for language in ["", "ts", "java", "py"] {
                controls.push(Control::Language(i, language));
            }
        } else {
            controls.push(Control::Field(i));
        }
    }
    controls.extend([Control::Submit, Control::Cancel]);
    controls
}

pub fn selected(form: &catalog::Form) -> Control {
    controls(form)
        .get(form.selected)
        .copied()
        .unwrap_or(Control::Submit)
}

pub fn field_index(form: &catalog::Form) -> Option<usize> {
    match selected(form) {
        Control::Field(i) => Some(i),
        _ => None,
    }
}

pub fn toggle_language(form: &mut catalog::Form, index: usize, language: &str) {
    let field = &mut form.fields[index];
    if language.is_empty() {
        field.value.clear();
    } else {
        let mut languages: Vec<_> = field.value.split(',').filter(|s| !s.is_empty()).collect();
        if languages.contains(&language) {
            languages.retain(|s| *s != language);
        } else {
            languages.push(language);
        }
        field.value = languages.join(",");
    }
}

fn checked(value: bool) -> &'static str {
    if value {
        "[x]"
    } else {
        "[ ]"
    }
}

fn action(form: &catalog::Form) -> &str {
    match form.name.as_str() {
        "index"
            if form
                .fields
                .iter()
                .any(|f| f.id == "force" && f.value == "true") =>
        {
            "Rebuild index"
        }
        "index" => "Build index",
        "config"
            if form
                .fields
                .iter()
                .any(|f| f.id == "key" && !f.value.is_empty()) =>
        {
            "Save settings"
        }
        "config" => "Show settings",
        "init" => "Initialize project",
        "analyze" => "Analyze",
        "export" => "Export",
        "entrypoints" => "Discover entrypoints",
        "label" | "label-step" => "Save labels",
        "clear" => "Clear data",
        _ => "Run",
    }
}

pub fn draw(frame: &mut Frame, form: &mut catalog::Form, root: &Path) -> Vec<(Rect, usize)> {
    let controls = controls(form);
    let field_count = controls.len() - 2;
    let area = popup(frame.area(), 100, (field_count as u16 + 13).max(16));
    frame.render_widget(Clear, area);
    let title = if form.name == "index" {
        "Build / refresh index"
    } else {
        &form.name
    };
    frame.render_widget(panel(&format!(" {title} · options ")), area);
    let inner = area.inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    let compact = area.height < 16;
    let regions = Layout::vertical([
        Constraint::Length(if compact { 0 } else { 2 }),
        Constraint::Min(1),
        Constraint::Length(if compact { 1 } else { 3 }),
        Constraint::Length(if compact { 0 } else { 1 }),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let about = if form.name == "index" {
        "Choose which languages to index. Click a checkbox or press Space to change it."
    } else {
        &form.about
    };
    frame.render_widget(
        Paragraph::new(clean(about)).wrap(Wrap { trim: false }),
        regions[0],
    );
    let items: Vec<_> = controls[..field_count]
        .iter()
        .map(|control| {
            let text = match *control {
                Control::Language(i, language) => {
                    let value = &form.fields[i].value;
                    let enabled = if language.is_empty() {
                        value.is_empty()
                    } else {
                        value.split(',').any(|s| s == language)
                    };
                    let label = match language {
                        "" => "Use project languages",
                        "ts" => "TypeScript / JavaScript",
                        "java" => "Java",
                        _ => "Python",
                    };
                    format!("{} {label}", checked(enabled))
                }
                Control::Field(i) => {
                    let field = &form.fields[i];
                    let label = if form.name == "index" && field.id == "force" {
                        "Force rebuild (ignore cache)"
                    } else {
                        field.long.as_deref().unwrap_or(&field.id)
                    };
                    if field.toggle {
                        format!("{} {label}", checked(field.value == "true"))
                    } else {
                        let value = if field.value.is_empty() {
                            "<default / omitted>".into()
                        } else {
                            clean(&field.value).replace('\n', " ")
                        };
                        format!("{label}{}: {value}", if field.required { " *" } else { "" })
                    }
                }
                _ => unreachable!(),
            };
            ListItem::new(text)
        })
        .collect();
    let focused = form.selected < field_count;
    let mut state =
        ListState::default().with_selected(Some(form.selected.min(field_count.saturating_sub(1))));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol(if focused { "› " } else { "  " })
            .highlight_style(if focused {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default()
            }),
        regions[1],
        &mut state,
    );
    form.offset = state.offset();
    let mut targets = Vec::new();
    for row in 0..regions[1].height {
        let index = state.offset() + row as usize;
        if index < field_count {
            targets.push((
                Rect::new(regions[1].x, regions[1].y + row, regions[1].width, 1),
                index,
            ));
        }
    }
    let help = if !form.error.is_empty() {
        form.error.clone()
    } else {
        match selected(form) {
            Control::Language(_, _) => "Use project languages indexes all enabled languages. Individual choices limit this run; languages must also be enabled in Settings.".into(),
            Control::Field(i) => form.fields[i].help.clone(),
            Control::Submit => format!("{} with the selected options. Progress appears in Activity.", action(form)),
            Control::Cancel => "Close without running or saving changes.".into(),
        }
    };
    frame.render_widget(
        Paragraph::new(clean(&help))
            .style(Style::default().fg(if form.error.is_empty() {
                Color::Cyan
            } else {
                Color::Red
            }))
            .wrap(Wrap { trim: false }),
        regions[2],
    );
    frame.render_widget(
        Paragraph::new(clean(&form.equivalent(root))).dim(),
        regions[3],
    );
    let buttons = [format!("[ {} ]", action(form)), "[ Cancel ]".into()];
    let mut x = regions[4].x;
    for (i, text) in buttons.iter().enumerate() {
        let index = field_count + i;
        let width = (text.len() as u16).min(regions[4].right().saturating_sub(x));
        let rect = Rect::new(x, regions[4].y, width, 1);
        let style = if form.selected == index {
            Style::default().bg(Color::Cyan).fg(Color::Black).bold()
        } else {
            Style::default()
                .fg(if i == 0 { Color::Cyan } else { Color::Gray })
                .bold()
        };
        frame.render_widget(Paragraph::new(text.as_str()).style(style), rect);
        targets.push((rect, index));
        x += width + 2;
    }
    frame.render_widget(
        Paragraph::new(if compact {
            "Tab move · Space toggle · F5 run"
        } else {
            "Tab / ↑↓ move · Space toggle · Enter activate · F5 run · Esc cancel"
        })
        .dim(),
        regions[5],
    );
    targets
}
