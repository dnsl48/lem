//! Terminal queries, answered on the input stream (ADR 0021).
//!
//! The terminal answers in the stream that carries keys. So a query reads
//! every event itself, unfiltered: replies decide the answer, and anything
//! else is set aside, in order, for the main loop.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::time::{Duration, Instant};

use termina::Event;
use termina::escape::csi::{Csi, Device, Keyboard, KittyKeyboardFlags};

use crate::ansi;
use crate::events::Events;

/// The terminal's active kitty keyboard flags: `Some` when it reported
/// them, `None` when it answered DA1 without them, so has no protocol.
///
/// Waits at most `timeout`. Flags with no DA1 by then are still the
/// answer; no reply at all is `TimedOut`. Events that are not replies go
/// to `pending`, as they came. A read error is returned, and what was set
/// aside before it stays set aside.
pub fn keyboard_flags(
    out: &mut impl Write,
    events: &mut impl Events,
    pending: &mut VecDeque<Event>,
    timeout: Duration,
) -> io::Result<Option<KittyKeyboardFlags>> {
    let started = Instant::now();
    let timed_out = || {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "the terminal sent no keyboard flags or DA1 before the deadline",
        )
    };
    if timeout.is_zero() {
        return Err(timed_out());
    }
    out.write_all(ansi::QUERY_KEYBOARD.as_bytes())?;
    out.flush()?;

    let mut flags = None;
    loop {
        let remaining = timeout.saturating_sub(started.elapsed());
        let event = if remaining.is_zero() {
            None
        } else {
            events.next(remaining)?
        };
        match event {
            None => return flags.map(Some).ok_or_else(timed_out),
            Some(Event::Csi(Csi::Keyboard(Keyboard::ReportFlags(reported)))) => {
                flags = Some(reported);
            }
            // DA1 answers last, so the flags, if any, came before it.
            Some(Event::Csi(Csi::Device(Device::DeviceAttributes(_)))) => return Ok(flags),
            Some(other) => pending.push_back(other),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::script::{Script, ctrl, key};

    const BUDGET: Duration = Duration::from_secs(1);

    fn flags(bits: u8) -> Event {
        Event::Csi(Csi::Keyboard(Keyboard::ReportFlags(
            KittyKeyboardFlags::from_bits_truncate(bits),
        )))
    }

    fn da1() -> Event {
        Event::Csi(Csi::Device(Device::DeviceAttributes(())))
    }

    fn arriving(events: impl IntoIterator<Item = Event>) -> Script {
        Script::of(events.into_iter().map(|event| Ok(Some(event))))
    }

    #[test]
    fn flags_then_da1_are_the_answer_and_the_query_is_written_once() {
        let mut out = Vec::new();
        let answer = keyboard_flags(
            &mut out,
            &mut arriving([flags(5), da1()]),
            &mut VecDeque::new(),
            BUDGET,
        )
        .unwrap();
        assert_eq!(answer, Some(KittyKeyboardFlags::from_bits_truncate(5)));
        assert_eq!(out, b"\x1b[?u\x1b[c");
    }

    #[test]
    fn da1_alone_means_no_protocol() {
        let answer = keyboard_flags(
            &mut Vec::new(),
            &mut arriving([da1()]),
            &mut VecDeque::new(),
            BUDGET,
        )
        .unwrap();
        assert_eq!(answer, None);
    }

    #[test]
    fn silence_is_a_timeout() {
        let error = keyboard_flags(
            &mut Vec::new(),
            &mut Script::of([]),
            &mut VecDeque::new(),
            BUDGET,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn flags_without_da1_are_still_the_answer() {
        let answer = keyboard_flags(
            &mut Vec::new(),
            &mut arriving([flags(1)]),
            &mut VecDeque::new(),
            BUDGET,
        )
        .unwrap();
        assert_eq!(answer, Some(KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES));
    }

    #[test]
    fn everything_else_is_set_aside_in_order_and_nothing_after_da1_is_read() {
        let mut script = arriving([key('a'), ctrl('5'), flags(0), key('b'), da1(), key('c')]);
        let mut pending = VecDeque::new();
        let answer = keyboard_flags(&mut Vec::new(), &mut script, &mut pending, BUDGET).unwrap();
        assert_eq!(answer, Some(KittyKeyboardFlags::empty()));
        assert_eq!(pending, [key('a'), ctrl('5'), key('b')]);
        assert_eq!(script.left(), 1, "the key after DA1 is the main loop's");
    }

    #[test]
    fn a_read_error_is_returned_and_what_was_set_aside_stays() {
        let mut script = Script::of([Ok(Some(key('a'))), Err(io::Error::other("gone"))]);
        let mut pending = VecDeque::new();
        assert!(keyboard_flags(&mut Vec::new(), &mut script, &mut pending, BUDGET).is_err());
        assert_eq!(pending, [key('a')]);
    }

    #[test]
    fn no_budget_asks_nothing() {
        let mut out = Vec::new();
        let error = keyboard_flags(
            &mut out,
            &mut Script::of([]),
            &mut VecDeque::new(),
            Duration::ZERO,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(out.is_empty());
    }
}
