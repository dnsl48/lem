//! Bounded, confirmed keyboard enhancement on the controlling terminal.
//!
//! crossterm remains the only input reader: its filtered query keeps keys
//! typed during negotiation queued for the ordinary event loop.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::Command;
use crossterm::event::{
    KeyboardEnhancementFlags as Flags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use lem_protocol::v1::TerminalCapabilities;

const BUDGET: Duration = Duration::from_secs(1);

/// The flag shared with terminal cleanup records completed push commands,
/// rather than attempts that may have written no complete command.
pub fn negotiate(out: &mut impl Write, pushed: &AtomicBool, legacy: bool) -> TerminalCapabilities {
    let started = Instant::now();
    negotiate_with(
        out,
        pushed,
        legacy,
        crossterm::terminal::query_keyboard_enhancement_flags_with_timeout,
        || started.elapsed(),
    )
}

fn negotiate_with(
    out: &mut impl Write,
    pushed: &AtomicBool,
    legacy: bool,
    mut query: impl FnMut(Duration) -> io::Result<Option<Flags>>,
    mut elapsed: impl FnMut() -> Duration,
) -> TerminalCapabilities {
    let legacy_capabilities = TerminalCapabilities::default();
    if legacy {
        return legacy_capabilities;
    }

    let mut remaining = || BUDGET.saturating_sub(elapsed());
    let timeout = remaining();
    if timeout.is_zero() || !matches!(query(timeout), Ok(Some(_))) {
        return legacy_capabilities;
    }
    // Support detection can consume the whole budget. Avoid pushing an
    // enhancement we already know we will have no time to confirm.
    if remaining().is_zero() {
        return legacy_capabilities;
    }

    let requested = Flags::DISAMBIGUATE_ESCAPE_CODES | Flags::REPORT_ALTERNATE_KEYS;
    if write_stack_command(out, pushed, PushKeyboardEnhancementFlags(requested), true).is_ok() {
        let timeout = remaining();
        if !timeout.is_zero()
            && let Ok(Some(active)) = query(timeout)
            && active.contains(requested)
            && !remaining().is_zero()
        {
            return TerminalCapabilities {
                keyboard_disambiguation: true,
                alternate_key_reporting: true,
                // The protocol can encode provenance, but each
                // event's keypad bit remains authoritative.
                keypad_identity: true,
            };
        }
    }

    // Balance a completed push, including one whose flush failed. An
    // incomplete command creates no stack entry we are entitled to pop.
    let _ = pop_if_owned(out, pushed);
    legacy_capabilities
}

/// Release only our completed push. A completed pop releases ownership
/// before flushing, so a flush error cannot make cleanup pop twice.
pub fn pop_if_owned(out: &mut impl Write, pushed: &AtomicBool) -> io::Result<()> {
    if pushed.load(Ordering::Acquire) {
        write_stack_command(out, pushed, PopKeyboardEnhancementFlags, false)
    } else {
        Ok(())
    }
}

fn write_stack_command(
    out: &mut impl Write,
    pushed: &AtomicBool,
    command: impl Command,
    owned_after_write: bool,
) -> io::Result<()> {
    // Format first so write_all completion precisely marks the complete
    // terminal command, separately from the subsequent flush result.
    let mut bytes = String::new();
    command.write_ansi(&mut bytes).map_err(io::Error::other)?;
    out.write_all(bytes.as_bytes())?;
    pushed.store(owned_after_write, Ordering::Release);
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn active() -> Flags {
        Flags::DISAMBIGUATE_ESCAPE_CODES | Flags::REPORT_ALTERNATE_KEYS
    }

    #[test]
    fn support_and_confirmation_share_one_budget() {
        let mut out = Vec::new();
        let pushed = AtomicBool::new(false);
        let elapsed = Cell::new(Duration::ZERO);
        let mut calls = Vec::new();
        let capabilities = negotiate_with(
            &mut out,
            &pushed,
            false,
            |timeout| {
                calls.push(timeout);
                if calls.len() == 1 {
                    elapsed.set(Duration::from_millis(750));
                    Ok(Some(Flags::empty()))
                } else {
                    elapsed.set(Duration::from_millis(900));
                    Ok(Some(active()))
                }
            },
            || elapsed.get(),
        );
        assert_eq!(calls, [BUDGET, Duration::from_millis(250)]);
        assert!(capabilities.keyboard_disambiguation);
        assert!(capabilities.alternate_key_reporting);
        assert!(capabilities.keypad_identity);
        assert!(pushed.load(Ordering::Acquire));
        assert_eq!(out, b"\x1b[>5u");
    }

    #[test]
    fn explicit_legacy_does_not_query_or_write() {
        let mut out = Vec::new();
        let capabilities = negotiate_with(
            &mut out,
            &AtomicBool::new(false),
            true,
            |_| panic!("legacy must not query"),
            || Duration::ZERO,
        );
        assert_eq!(capabilities, TerminalCapabilities::default());
        assert!(out.is_empty());
    }

    #[test]
    fn unsupported_and_failed_queries_are_legacy_without_a_push() {
        for reply in [
            Ok(None),
            Err(io::Error::new(io::ErrorKind::TimedOut, "no reply")),
            Err(io::Error::new(io::ErrorKind::InvalidData, "bad reply")),
        ] {
            let mut reply = Some(reply);
            let mut out = Vec::new();
            let pushed = AtomicBool::new(false);
            let capabilities = negotiate_with(
                &mut out,
                &pushed,
                false,
                |_| reply.take().unwrap(),
                || Duration::ZERO,
            );
            assert_eq!(capabilities, TerminalCapabilities::default());
            assert!(!pushed.load(Ordering::Acquire));
            assert!(out.is_empty());
        }
    }

    #[test]
    fn detection_exhausting_the_budget_does_not_push() {
        let mut out = Vec::new();
        let elapsed = Cell::new(Duration::ZERO);
        let capabilities = negotiate_with(
            &mut out,
            &AtomicBool::new(false),
            false,
            |_| {
                elapsed.set(BUDGET);
                Ok(Some(Flags::empty()))
            },
            || elapsed.get(),
        );
        assert_eq!(capabilities, TerminalCapabilities::default());
        assert!(out.is_empty());
    }

    #[test]
    fn unconfirmed_push_is_popped_and_not_advertised() {
        for confirmation in [
            Ok(None),
            Ok(Some(Flags::empty())),
            Ok(Some(Flags::DISAMBIGUATE_ESCAPE_CODES)),
            Err(io::Error::new(io::ErrorKind::TimedOut, "no reply")),
        ] {
            let mut replies = [Ok(Some(Flags::empty())), confirmation].into_iter();
            let mut out = Vec::new();
            let pushed = AtomicBool::new(false);
            let capabilities = negotiate_with(
                &mut out,
                &pushed,
                false,
                |_| replies.next().unwrap(),
                || Duration::ZERO,
            );
            assert_eq!(capabilities, TerminalCapabilities::default());
            assert!(!pushed.load(Ordering::Acquire));
            assert_eq!(out, b"\x1b[>5u\x1b[<1u");
        }
    }

    #[test]
    fn incomplete_push_does_not_pop_an_outer_keyboard_stack() {
        for accepted_bytes in [0, 2, 4] {
            let mut out = FaultyOutput {
                fail_write_at: Some(accepted_bytes),
                ..Default::default()
            };
            let pushed = AtomicBool::new(false);
            let mut queries = 0;
            let capabilities = negotiate_with(
                &mut out,
                &pushed,
                false,
                |_| {
                    queries += 1;
                    Ok(Some(Flags::empty()))
                },
                || Duration::ZERO,
            );
            assert_eq!(queries, 1);
            assert_eq!(capabilities, TerminalCapabilities::default());
            assert_eq!(out.bytes, &b"\x1b[>5u"[..accepted_bytes]);
            assert!(!pushed.load(Ordering::Acquire));
            pop_if_owned(&mut out, &pushed).unwrap();
            assert_eq!(out.bytes.len(), accepted_bytes);
        }
    }

    #[test]
    fn completed_push_with_failed_flush_is_balanced_once() {
        let mut out = FaultyOutput {
            fail_flush_on: Some(1),
            ..Default::default()
        };
        let pushed = AtomicBool::new(false);
        let mut queries = 0;
        let capabilities = negotiate_with(
            &mut out,
            &pushed,
            false,
            |_| {
                queries += 1;
                Ok(Some(Flags::empty()))
            },
            || Duration::ZERO,
        );
        assert_eq!(queries, 1);
        assert_eq!(capabilities, TerminalCapabilities::default());
        assert!(!pushed.load(Ordering::Acquire));
        pop_if_owned(&mut out, &pushed).unwrap();
        assert_eq!(out.bytes, b"\x1b[>5u\x1b[<1u");
        assert_eq!(out.flushes, 2);
    }

    #[test]
    fn completed_pop_with_failed_flush_is_not_retried() {
        let mut out = FaultyOutput {
            fail_flush_on: Some(2),
            ..Default::default()
        };
        let pushed = AtomicBool::new(false);
        let capabilities = negotiate_with(
            &mut out,
            &pushed,
            false,
            |_| Ok(Some(Flags::empty())),
            || Duration::ZERO,
        );
        assert_eq!(capabilities, TerminalCapabilities::default());
        assert!(!pushed.load(Ordering::Acquire));
        pop_if_owned(&mut out, &pushed).unwrap();
        assert_eq!(out.bytes, b"\x1b[>5u\x1b[<1u");
        assert_eq!(out.flushes, 2);
    }

    #[test]
    fn incomplete_pop_keeps_cleanup_ownership() {
        let mut out = FaultyOutput {
            fail_write_at: Some(7),
            ..Default::default()
        };
        let pushed = AtomicBool::new(false);
        let capabilities = negotiate_with(
            &mut out,
            &pushed,
            false,
            |_| Ok(Some(Flags::empty())),
            || Duration::ZERO,
        );
        assert_eq!(capabilities, TerminalCapabilities::default());
        assert!(pushed.load(Ordering::Acquire));
        assert_eq!(out.bytes, b"\x1b[>5u\x1b[");
        pop_if_owned(&mut out, &pushed).unwrap();
        assert!(!pushed.load(Ordering::Acquire));
        assert_eq!(out.bytes, b"\x1b[>5u\x1b[\x1b[<1u");
    }

    #[derive(Default)]
    struct FaultyOutput {
        bytes: Vec<u8>,
        fail_write_at: Option<usize>,
        fail_flush_on: Option<usize>,
        flushes: usize,
    }

    impl Write for FaultyOutput {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let length = if let Some(limit) = self.fail_write_at {
                if self.bytes.len() == limit {
                    self.fail_write_at = None;
                    return Err(io::Error::new(io::ErrorKind::BrokenPipe, "failed write"));
                }
                bytes.len().min(limit - self.bytes.len())
            } else {
                bytes.len()
            };
            self.bytes.extend_from_slice(&bytes[..length]);
            Ok(length)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            if self.fail_flush_on == Some(self.flushes) {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "failed flush"))
            } else {
                Ok(())
            }
        }
    }
}
