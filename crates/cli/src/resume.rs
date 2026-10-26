//! Resuming earlier conversations (`ctx --continue`, `ctx --resume [id]`,
//! `/resume`): the sessions of this repository worth going back to, found
//! by id prefix or picked from a list.

use crate::transcript::theme::*;
use crate::transcript::truncate_end;
use ctx_core::Result;
use ctx_storage::{SessionRecord, SessionStore};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;

/// Sessions the picker lists at most.
const LIST_LIMIT: usize = 200;

/// One conversation in the resume list.
pub struct SessionChoice {
    pub record: SessionRecord,
    /// Its title (desktop rename), else its first prompt.
    pub title: String,
    pub prompts: u64,
    pub tasks: usize,
    pub last_status: Option<String>,
}

/// Conversations of this repository with at least one task, most recently
/// active first; `exclude` (the open session) is left out.
pub fn list(store: &SessionStore, repository_id: &str, exclude: Option<&str>) -> Result<Vec<SessionChoice>> {
    let mut out = Vec::new();
    for record in store.list_sessions(repository_id, LIST_LIMIT)? {
        if Some(record.id.as_str()) == exclude {
            continue;
        }
        let tasks = store.tasks_for_session(&record.id)?;
        if tasks.is_empty() {
            continue;
        }
        let title = record
            .metadata
            .get("title")
            .and_then(|t| t.as_str())
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| tasks[0].objective.split_whitespace().collect::<Vec<_>>().join(" "));
        let last_status = tasks.iter().max_by_key(|t| t.updated_at).and_then(|t| t.status.clone());
        let prompts = store.user_message_count(&record.id)?;
        out.push(SessionChoice { title, prompts, tasks: tasks.len(), last_status, record });
    }
    Ok(out)
}

/// A session by its id or the start of it (`ctx --resume 6f3a9c2e`).
pub fn find(store: &SessionStore, repository_id: &str, id: &str) -> Result<Option<SessionRecord>> {
    let id = id.trim();
    if let Some(s) = store.get_session(id)?.filter(|s| s.mode != SessionStore::AGENT_MODE) {
        return Ok(Some(s));
    }
    let matches: Vec<SessionRecord> = store.list_sessions(repository_id, usize::MAX >> 1)?.into_iter().filter(|s| s.id.starts_with(id)).collect();
    Ok(if matches.len() == 1 { matches.into_iter().next() } else { None })
}

/// `just now`, `5m ago`, `3h ago`, `2d ago`, then the date.
pub fn fmt_ago(at: chrono::DateTime<chrono::Utc>) -> String {
    let secs = (chrono::Utc::now() - at).num_seconds().max(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86_399 => format!("{}h ago", secs / 3600),
        86_400..=604_799 => format!("{}d ago", secs / 86_400),
        _ => at.with_timezone(&chrono::Local).format("%b %-d, %Y").to_string(),
    }
}

/// The `/resume` popup: the conversations, narrowed by a typed filter
/// that matches titles and ids.
pub struct ResumePicker {
    pub sessions: Vec<SessionChoice>,
    pub filter: String,
    pub selected: usize,
}

impl ResumePicker {
    pub fn new(sessions: Vec<SessionChoice>, filter: &str) -> Self {
        Self { sessions, filter: filter.to_string(), selected: 0 }
    }

    pub fn matches(&self) -> Vec<&SessionChoice> {
        let needle = self.filter.trim().to_lowercase();
        self.sessions.iter().filter(|s| needle.is_empty() || s.title.to_lowercase().contains(&needle) || s.record.id.starts_with(&needle)).collect()
    }

    pub fn choice(&self) -> Option<SessionRecord> {
        self.matches().get(self.selected).map(|s| s.record.clone())
    }
}

pub fn draw_picker(f: &mut Frame, picker: &ResumePicker) {
    let full = f.area();
    let area = centered(full, 80, 80);
    f.render_widget(Clear, area);
    let matches = picker.matches();
    let title = format!(" resume a conversation · {} of {} ", matches.len(), picker.sessions.len());
    let block = Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(ACCENT)).title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let [header, list, footer] = Layout::vertical([Constraint::Length(2), Constraint::Min(1), Constraint::Length(1)]).areas(inner);
    let dim = Style::default().fg(DIM);
    f.render_widget(Paragraph::new(Line::from(vec![Span::styled("search ", dim), Span::raw(picker.filter.as_str())])), header);
    f.set_cursor_position((header.x + 7 + picker.filter.chars().count() as u16, header.y));

    let width = list.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    if picker.sessions.is_empty() {
        lines.push(Line::styled("no earlier conversations in this repository", dim));
    } else if matches.is_empty() {
        lines.push(Line::styled("no conversation matches", dim));
    }
    // Two rows per conversation; keep the selected one on screen.
    let per_page = ((list.height as usize).saturating_sub(lines.len()) / 2).max(1);
    let offset = picker.selected.saturating_sub(per_page - 1);
    for (i, s) in matches.iter().enumerate().skip(offset).take(per_page) {
        let on = i == picker.selected;
        let when = fmt_ago(s.record.updated_at);
        let prompts = format!("{} prompt{}", s.prompts, if s.prompts == 1 { "" } else { "s" });
        let right = format!("{when} · {prompts}");
        let room = width.saturating_sub(right.chars().count() + 5);
        let title = truncate_end(&s.title, room);
        let pad = width.saturating_sub(2 + title.chars().count() + right.chars().count() + 1);
        let marker = if on { Span::styled("› ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)) } else { Span::raw("  ") };
        let name = if on { Style::default().fg(ACCENT).add_modifier(Modifier::BOLD) } else { Style::default().add_modifier(Modifier::BOLD) };
        let top = Line::from(vec![marker, Span::styled(title, name), Span::raw(" ".repeat(pad)), Span::styled(right, dim)]);
        let mut meta = vec![s.record.id.chars().take(8).collect::<String>()];
        if s.record.mode != "interactive" {
            meta.push(s.record.mode.clone());
        }
        meta.push(s.record.model.clone());
        meta.push(format!("{} task{}", s.tasks, if s.tasks == 1 { "" } else { "s" }));
        let status = s.last_status.clone();
        let mut bottom = vec![Span::raw("  "), Span::styled(truncate_end(&meta.join(" · "), width.saturating_sub(16)), Style::default().fg(FAINT))];
        if let Some(status) = status {
            let color = match status.as_str() {
                "completed" => OK,
                "failed" => ERR,
                "blocked" => WARN,
                _ => DIM,
            };
            bottom.push(Span::styled(format!(" · {status}"), Style::default().fg(color)));
        }
        let bottom = Line::from(bottom);
        if on {
            lines.push(top.style(Style::default().bg(SELECTED_BG)));
            lines.push(bottom.style(Style::default().bg(SELECTED_BG)));
        } else {
            lines.push(top);
            lines.push(bottom);
        }
    }
    f.render_widget(Paragraph::new(lines), list);
    f.render_widget(Paragraph::new(Line::styled("↑↓ select · Enter resume · Esc cancel", dim)), footer);
}

fn centered(area: Rect, pct_x: u16, pct_y: u16) -> Rect {
    let [_, mid, _] = Layout::vertical([Constraint::Percentage((100 - pct_y) / 2), Constraint::Percentage(pct_y), Constraint::Percentage((100 - pct_y) / 2)]).areas(area);
    let [_, cell, _] = Layout::horizontal([Constraint::Percentage((100 - pct_x) / 2), Constraint::Percentage(pct_x), Constraint::Percentage((100 - pct_x) / 2)]).areas(mid);
    cell
}
