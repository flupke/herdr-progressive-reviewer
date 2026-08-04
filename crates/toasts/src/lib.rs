//! Toast state and rendering.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};
use uuid::Uuid;

const LONG_TOAST_DELAY: Duration = Duration::from_millis(250);

/// Identifies one toast across asynchronous work.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ToastId(Uuid);

impl ToastId {
    /// Generate a new toast ID.
    pub fn generate() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Toast {
    text: String,
    expires: Instant,
    kind: ToastKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LongToast {
    id: ToastId,
    text: String,
    started: Instant,
}

/// Active short and long toasts.
#[derive(Debug, Default)]
pub struct ToastState {
    toasts: Vec<Toast>,
    long_toasts: VecDeque<LongToast>,
}

/// Visual severity of a toast.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToastKind {
    Info,
    Error,
}

impl ToastKind {
    fn duration(self) -> Duration {
        match self {
            Self::Info => Duration::from_secs(3),
            Self::Error => Duration::from_secs(6),
        }
    }
}

impl ToastState {
    /// Add a short toast.
    pub fn push(&mut self, text: impl Into<String>, kind: ToastKind) {
        self.toasts.push(Toast {
            text: text.into(),
            expires: Instant::now() + kind.duration(),
            kind,
        });
    }

    /// Remove expired short toasts.
    pub fn expire(&mut self, now: Instant) {
        self.toasts.retain(|toast| toast.expires > now);
    }

    /// Add a toast that appears after 250 ms and remains until finished.
    pub fn start_long_toast(&mut self, text: impl Into<String>) -> ToastId {
        let id = ToastId::generate();
        self.long_toasts.push_back(LongToast {
            id,
            text: text.into(),
            started: Instant::now(),
        });
        id
    }

    /// Remove a long toast.
    pub fn finish_toast(&mut self, id: ToastId) {
        if let Some(index) = self.long_toasts.iter().position(|toast| toast.id == id) {
            self.long_toasts.remove(index);
        }
    }

    /// Render active toasts.
    pub fn render(&self, area: Rect, buffer: &mut Buffer, info: Color, error: Color) {
        self.render_at(area, buffer, info, error, Instant::now());
    }

    fn render_at(&self, area: Rect, buffer: &mut Buffer, info: Color, error: Color, now: Instant) {
        let long_toast = self
            .long_toasts
            .front()
            .filter(|toast| now.saturating_duration_since(toast.started) >= LONG_TOAST_DELAY);
        if let Some(toast) = long_toast {
            ToastView {
                text: &toast.text,
                kind: ToastKind::Info,
                stack_index: 0,
                info,
                error,
            }
            .render(area, buffer);
        }
        for (index, toast) in self
            .toasts
            .iter()
            .rev()
            .take(usize::from(area.height / 3).saturating_sub(usize::from(long_toast.is_some())))
            .enumerate()
        {
            ToastView {
                text: &toast.text,
                kind: toast.kind,
                stack_index: index + usize::from(long_toast.is_some()),
                info,
                error,
            }
            .render(area, buffer);
        }
    }
}

struct ToastView<'a> {
    text: &'a str,
    kind: ToastKind,
    stack_index: usize,
    info: Color,
    error: Color,
}

impl Widget for ToastView<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let width = u16::try_from(self.text.chars().count().saturating_add(4))
            .unwrap_or(area.width)
            .min(area.width);
        let offset = u16::try_from(self.stack_index.saturating_mul(3)).unwrap_or(u16::MAX);
        let popup = Rect::new(
            area.right().saturating_sub(width),
            area.bottom().saturating_sub(3).saturating_sub(offset),
            width,
            3.min(area.height),
        );
        Clear.render(popup, buffer);
        let color = match self.kind {
            ToastKind::Info => self.info,
            ToastKind::Error => self.error,
        };
        Paragraph::new(Line::from(vec![Span::styled(
            self.text,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(color)),
        )
        .render(popup, buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toasts_stack_delay_and_expire_independently() {
        let now = Instant::now();
        let mut toasts = ToastState::default();
        toasts.push("first", ToastKind::Info);
        toasts.push("second", ToastKind::Error);
        let long = toasts.start_long_toast("long");
        let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 12));

        toasts.render_at(buffer.area, &mut buffer, Color::Green, Color::Red, now);
        let content = buffer
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(content.contains("first"));
        assert!(content.contains("second"));
        assert!(!content.contains("long"));

        toasts.long_toasts.front_mut().unwrap().started =
            now.checked_sub(LONG_TOAST_DELAY).unwrap();
        toasts.render_at(buffer.area, &mut buffer, Color::Green, Color::Red, now);
        assert!(
            buffer
                .content
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .contains("long")
        );

        toasts.finish_toast(long);
        toasts.expire(now + Duration::from_secs(7));
        assert!(toasts.toasts.is_empty());
        assert!(toasts.long_toasts.is_empty());
    }
}
