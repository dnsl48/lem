//! The terminal's input, as the main loop reads it (ADR 0021).
//!
//! Keyboard negotiation reads the terminal before the main loop does and
//! sets aside what is not a reply. `Input` hands those over first, in
//! order, then the terminal's own events.

use std::collections::VecDeque;
use std::io;
use std::time::Duration;

use termina::{Event, EventReader};

use crate::input::{self, Encoding};

/// Somewhere terminal events come from.
pub trait Events {
    /// The next event, waiting at most `timeout`; `None` when none came.
    fn next(&mut self, timeout: Duration) -> io::Result<Option<Event>>;
}

impl Events for EventReader {
    fn next(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        // Unfiltered both times, so events come out in the order they
        // arrived: a filtered read moves the events it skips behind the
        // ones still queued.
        if self.poll(Some(timeout), |_| true)? {
            self.read(|_| true).map(Some)
        } else {
            Ok(None)
        }
    }
}

/// The main loop's terminal input.
pub struct Input<E> {
    events: E,
    pending: VecDeque<Event>,
    encoding: Encoding,
}

impl<E: Events> Input<E> {
    /// `pending` is what negotiation set aside, already decoded;
    /// `encoding` is the one negotiation settled on.
    pub fn new(events: E, pending: VecDeque<Event>, encoding: Encoding) -> Self {
        Self {
            events,
            pending,
            encoding,
        }
    }

    /// The next event, waiting at most `timeout` for the terminal.
    pub fn next(&mut self, timeout: Duration) -> io::Result<Option<Event>> {
        if let Some(event) = self.pending.pop_front() {
            return Ok(Some(event));
        }
        let mut event = self.events.next(timeout)?;
        if let Some(event) = event.as_mut() {
            input::decode(event, self.encoding);
        }
        Ok(event)
    }
}

/// Scripted terminal events, for this crate's tests.
#[cfg(test)]
pub mod script {
    use super::*;
    use termina::event::{KeyCode, KeyEvent, Modifiers};

    /// Each step answers one `next`; after the last, every wait ends with
    /// nothing, as a silent terminal's would.
    pub struct Script(VecDeque<io::Result<Option<Event>>>);

    impl Script {
        pub fn of(steps: impl IntoIterator<Item = io::Result<Option<Event>>>) -> Self {
            Self(steps.into_iter().collect())
        }

        /// Steps not read yet.
        pub fn left(&self) -> usize {
            self.0.len()
        }
    }

    impl Events for Script {
        fn next(&mut self, _timeout: Duration) -> io::Result<Option<Event>> {
            self.0.pop_front().unwrap_or(Ok(None))
        }
    }

    pub fn key(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), Modifiers::NONE))
    }

    pub fn ctrl(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), Modifiers::CONTROL))
    }
}

#[cfg(test)]
mod tests {
    use super::script::{Script, ctrl, key};
    use super::*;

    const WAIT: Duration = Duration::from_millis(10);

    fn live(event: Event) -> Script {
        Script::of([Ok(Some(event))])
    }

    #[test]
    fn set_aside_events_come_first_in_order() {
        let pending = VecDeque::from([key('a'), key('b')]);
        let mut input = Input::new(live(key('c')), pending, Encoding::Kitty);
        for expected in ['a', 'b', 'c'] {
            assert_eq!(input.next(WAIT).unwrap(), Some(key(expected)));
        }
        assert_eq!(input.next(WAIT).unwrap(), None);
    }

    #[test]
    fn under_legacy_encoding_live_c0_keys_are_named() {
        let mut input = Input::new(live(ctrl('5')), VecDeque::new(), Encoding::Legacy);
        assert_eq!(input.next(WAIT).unwrap(), Some(ctrl(']')));
    }

    #[test]
    fn under_the_kitty_protocol_ctrl_digits_stay_digits() {
        let mut input = Input::new(live(ctrl('5')), VecDeque::new(), Encoding::Kitty);
        assert_eq!(input.next(WAIT).unwrap(), Some(ctrl('5')));
    }

    #[test]
    fn set_aside_keys_are_not_decoded_again() {
        // Negotiation decoded it by when it was read (keyboard.rs).
        let pending = VecDeque::from([ctrl('5')]);
        let mut input = Input::new(Script::of([]), pending, Encoding::Legacy);
        assert_eq!(input.next(WAIT).unwrap(), Some(ctrl('5')));
    }

    #[test]
    fn errors_reach_the_caller() {
        let gone = io::Error::new(io::ErrorKind::UnexpectedEof, "terminal gone");
        let mut input = Input::new(Script::of([Err(gone)]), VecDeque::new(), Encoding::Kitty);
        assert_eq!(
            input.next(WAIT).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
}
