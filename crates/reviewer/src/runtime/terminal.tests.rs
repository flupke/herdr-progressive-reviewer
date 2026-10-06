use std::cell::RefCell;
use std::rc::Rc;

use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;
use ratatui::{Terminal, TerminalOptions, Viewport};

use super::*;
use crate::runtime::effects::fixture::EffectsFixture;
use crate::runtime::{
    ApplicationTick, EventEnvelope, PaneId, RuntimeEventLoop, TerminalEventProducer,
    TerminalFocused, UserInput, timing,
};
use herdr_client::protocol::HerdrEvent;

#[derive(Clone, Default)]
struct CapturedOutput(Rc<RefCell<Vec<u8>>>);

impl Write for CapturedOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Fixture {
    output: CapturedOutput,
    terminal: Terminal<TerminalBackend<CapturedOutput>>,
}

impl Fixture {
    fn new() -> Self {
        let output = CapturedOutput::default();
        let terminal = Terminal::with_options(
            TerminalBackend::new(output.clone()),
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 80, 20)),
            },
        )
        .unwrap();
        Self { output, terminal }
    }

    fn draw(&mut self, text: &str, style: Style) {
        self.terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new(text).style(style), frame.area());
            })
            .unwrap();
    }

    fn bytes_written(&self) -> usize {
        self.output.0.borrow().len()
    }

    fn handle_event(&mut self, event: &EventEnvelope) {
        let mut fixture = EffectsFixture::new(review_repository::repository::RepoType::Git);
        let _ = RuntimeEventLoop {
            effects: &mut fixture.effects,
            terminal_events: None,
            terminal: &mut self.terminal,
            app: &mut review_ui::ReviewApplication::default(),
            events: &mut fixture.inbox,
            timings: &timing::Recorder::default(),
            last_frame: std::time::Instant::now(),
        }
        .handle_event(event)
        .unwrap();
    }
}

#[test]
fn vision_mode_marks_each_completed_paint_including_unchanged_frames() {
    let output = CapturedOutput::default();
    let mut terminal = Terminal::with_options(
        TerminalBackend::new(output.clone()).with_frame_capture(),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 20, 3)),
        },
    )
    .unwrap();
    for _ in 0..2 {
        terminal
            .draw(|frame| frame.render_widget(Paragraph::new("visible"), frame.area()))
            .unwrap();
    }
    let bytes = output.0.borrow();
    let text = String::from_utf8_lossy(&bytes);
    assert_eq!(text.matches("\x1b[?2026h").count(), 2);
    assert_eq!(text.matches("\x1b[?2026l").count(), 2);
    assert!(text.ends_with("\x1b[?2026l"));
    assert!(text.find("\x1b[?2026h").unwrap() < text.find("visible").unwrap());
    assert!(text.find("visible").unwrap() < text.find("\x1b[?2026l").unwrap());
}

#[test]
fn unchanged_frames_produce_no_terminal_output() {
    let mut fixture = Fixture::new();
    fixture.draw("Review this file", Style::default());
    let initial_output = fixture.bytes_written();
    assert!(initial_output > 0);

    for _ in 0..100 {
        fixture.draw("Review this file", Style::default());
    }
    assert_eq!(fixture.bytes_written(), initial_output);

    fixture.draw("File reviewed", Style::default());
    let changed_output = fixture.bytes_written();
    assert!(changed_output > initial_output);
    fixture.draw("File reviewed", Style::default().fg(Color::Green));
    assert!(fixture.bytes_written() > changed_output);
}

#[test]
fn repaint_hides_an_externally_exposed_cursor_before_writing_cells() {
    let mut fixture = Fixture::new();
    fixture.draw("Review this file", Style::default());

    // A host cursor change does not pass through the backend's visibility cache.
    fixture.output.write_all(b"\x1b[?25h").unwrap();
    let before_repaint = fixture.bytes_written();
    fixture.draw("File reviewed", Style::default());
    let output = fixture.output.0.borrow();
    let repaint = &output[before_repaint..];
    assert!(repaint.starts_with(b"\x1b[?25l"));
    assert!(!repaint.windows(6).any(|bytes| bytes == b"\x1b[?25h"));
    drop(output);

    let after_repaint = fixture.bytes_written();
    fixture.draw("File reviewed", Style::default());
    assert_eq!(fixture.bytes_written(), after_repaint);
}

#[test]
fn clicks_and_focus_hide_an_exposed_cursor_without_a_changed_frame() {
    let mut fixture = Fixture::new();
    fixture.draw("Review this file", Style::default());
    for event in [
        EventEnvelope::new(UserInput::MouseClick { column: 0, row: 0 }),
        EventEnvelope::new(HerdrEvent::PaneFocused(PaneId("review-pane".into()))),
        EventEnvelope::new(TerminalFocused),
    ] {
        fixture.output.write_all(b"\x1b[?25h").unwrap();
        let before_input = fixture.bytes_written();
        fixture.handle_event(&event);
        fixture.draw("Review this file", Style::default());
        assert_eq!(&fixture.output.0.borrow()[before_input..], b"\x1b[?25l");

        let after_input = fixture.bytes_written();
        fixture.handle_event(&EventEnvelope::new(ApplicationTick(
            std::time::Instant::now(),
        )));
        fixture.draw("Review this file", Style::default());
        assert_eq!(fixture.bytes_written(), after_input);
    }
}

#[test]
fn terminal_focus_reaches_the_event_loop() {
    let (sender, receiver) = crossbeam_channel::unbounded();
    let mut input = Some(crossterm::event::Event::FocusGained);
    let producer = TerminalEventProducer::start_with_reader(sender, move |_| Ok(input.take()));
    let event = receiver
        .recv_timeout(std::time::Duration::from_secs(30))
        .unwrap();
    producer.stop();
    assert!(event.downcast_ref::<TerminalFocused>().is_some());
}

#[test]
fn a_frame_can_still_request_a_visible_cursor_after_painting() {
    let mut fixture = Fixture::new();
    fixture
        .terminal
        .draw(|frame| {
            frame.render_widget(Paragraph::new("Input"), frame.area());
            frame.set_cursor_position((5, 0));
        })
        .unwrap();
    let output = fixture.output.0.borrow();
    assert!(output.starts_with(b"\x1b[?25l"));
    assert!(output.windows(6).any(|bytes| bytes == b"\x1b[?25h"));
    drop(output);

    let before_hiding = fixture.bytes_written();
    fixture.draw("Input", Style::default());
    assert_eq!(&fixture.output.0.borrow()[before_hiding..], b"\x1b[?25l");
}

#[test]
fn resize_and_cursor_transitions_still_reach_the_terminal() {
    let mut fixture = Fixture::new();
    fixture.draw("Review", Style::default());
    let initial_output = fixture.bytes_written();
    fixture.terminal.resize(Rect::new(0, 0, 100, 30)).unwrap();
    fixture.draw("Review", Style::default());
    assert!(fixture.bytes_written() > initial_output);

    let resized_output = fixture.bytes_written();
    fixture.terminal.show_cursor().unwrap();
    let visible_output = fixture.bytes_written();
    assert!(visible_output > resized_output);
    fixture.terminal.hide_cursor().unwrap();
    let hidden_output = fixture.bytes_written();
    assert!(hidden_output > visible_output);
    fixture.terminal.hide_cursor().unwrap();
    assert_eq!(fixture.bytes_written(), hidden_output);

    // Commands written outside Backend invalidate the cached cursor state.
    fixture
        .terminal
        .backend_mut()
        .write_all(b"\x1b[?25h")
        .unwrap();
    let raw_output = fixture.bytes_written();
    fixture.draw("Review", Style::default());
    assert!(fixture.bytes_written() > raw_output);
}

#[test]
fn a_copy_writes_the_text_to_the_clipboard_selection_with_osc_52() {
    let output = CapturedOutput::default();
    let mut backend = TerminalBackend::new(output.clone());

    PaneBackend::copy_to_clipboard(&mut backend, "http://10.0.0.2:8790/").unwrap();

    // Herdr takes OSC 52 writes to the `c` selection from a pane; the text is in base64.
    assert_eq!(
        String::from_utf8(output.0.borrow().clone()).unwrap(),
        "\u{1b}]52;c;aHR0cDovLzEwLjAuMC4yOjg3OTAv\u{1b}\\"
    );
}
