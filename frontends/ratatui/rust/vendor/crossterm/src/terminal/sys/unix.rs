//! UNIX related logic for terminal manipulation.

#[cfg(feature = "events")]
use crate::event::KeyboardEnhancementFlags;
use crate::terminal::{
    sys::file_descriptor::{tty_fd, FileDesc},
    WindowSize,
};
#[cfg(feature = "libc")]
use libc::{
    cfmakeraw, ioctl, tcgetattr, tcsetattr, termios as Termios, winsize, STDOUT_FILENO, TCSANOW,
    TIOCGWINSZ,
};
use parking_lot::Mutex;
#[cfg(not(feature = "libc"))]
use rustix::{
    fd::AsFd,
    termios::{Termios, Winsize},
};

use std::{fs::File, io, process};
#[cfg(feature = "libc")]
use std::{
    mem,
    os::unix::io::{IntoRawFd, RawFd},
};

// Some(Termios) -> we're in the raw mode and this is the previous mode
// None -> we're not in the raw mode
static TERMINAL_MODE_PRIOR_RAW_MODE: Mutex<Option<Termios>> = parking_lot::const_mutex(None);

pub(crate) fn is_raw_mode_enabled() -> bool {
    TERMINAL_MODE_PRIOR_RAW_MODE.lock().is_some()
}

#[cfg(feature = "libc")]
impl From<winsize> for WindowSize {
    fn from(size: winsize) -> WindowSize {
        WindowSize {
            columns: size.ws_col,
            rows: size.ws_row,
            width: size.ws_xpixel,
            height: size.ws_ypixel,
        }
    }
}
#[cfg(not(feature = "libc"))]
impl From<Winsize> for WindowSize {
    fn from(size: Winsize) -> WindowSize {
        WindowSize {
            columns: size.ws_col,
            rows: size.ws_row,
            width: size.ws_xpixel,
            height: size.ws_ypixel,
        }
    }
}

#[allow(clippy::useless_conversion)]
#[cfg(feature = "libc")]
pub(crate) fn window_size() -> io::Result<WindowSize> {
    // http://rosettacode.org/wiki/Terminal_control/Dimensions#Library:_BSD_libc
    let mut size = winsize {
        ws_row: 0,
        ws_col: 0,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    let file = File::open("/dev/tty").map(|file| (FileDesc::new(file.into_raw_fd(), true)));
    let fd = if let Ok(file) = &file {
        file.raw_fd()
    } else {
        // Fallback to libc::STDOUT_FILENO if /dev/tty is missing
        STDOUT_FILENO
    };

    if wrap_with_result(unsafe { ioctl(fd, TIOCGWINSZ.into(), &mut size) }).is_ok() {
        return Ok(size.into());
    }

    Err(std::io::Error::last_os_error().into())
}

#[cfg(not(feature = "libc"))]
pub(crate) fn window_size() -> io::Result<WindowSize> {
    let file = File::open("/dev/tty").map(|file| (FileDesc::Owned(file.into())));
    let fd = if let Ok(file) = &file {
        file.as_fd()
    } else {
        // Fallback to libc::STDOUT_FILENO if /dev/tty is missing
        rustix::stdio::stdout()
    };
    let size = rustix::termios::tcgetwinsize(fd)?;
    Ok(size.into())
}

#[allow(clippy::useless_conversion)]
pub(crate) fn size() -> io::Result<(u16, u16)> {
    if let Ok(window_size) = window_size() {
        return Ok((window_size.columns, window_size.rows));
    }

    tput_size().ok_or_else(|| std::io::Error::last_os_error().into())
}

#[cfg(feature = "libc")]
pub(crate) fn enable_raw_mode() -> io::Result<()> {
    let mut original_mode = TERMINAL_MODE_PRIOR_RAW_MODE.lock();
    if original_mode.is_some() {
        return Ok(());
    }

    let tty = tty_fd()?;
    let fd = tty.raw_fd();
    let mut ios = get_terminal_attr(fd)?;
    let original_mode_ios = ios;
    raw_terminal_attr(&mut ios);
    set_terminal_attr(fd, &ios)?;
    // Keep it last - set the original mode only if we were able to switch to the raw mode
    *original_mode = Some(original_mode_ios);
    Ok(())
}

#[cfg(not(feature = "libc"))]
pub(crate) fn enable_raw_mode() -> io::Result<()> {
    let mut original_mode = TERMINAL_MODE_PRIOR_RAW_MODE.lock();
    if original_mode.is_some() {
        return Ok(());
    }

    let tty = tty_fd()?;
    let mut ios = get_terminal_attr(&tty)?;
    let original_mode_ios = ios.clone();
    ios.make_raw();
    set_terminal_attr(&tty, &ios)?;
    // Keep it last - set the original mode only if we were able to switch to the raw mode
    *original_mode = Some(original_mode_ios);
    Ok(())
}

/// Reset the raw mode.
///
/// More precisely, reset the whole termios mode to what it was before the first call
/// to [enable_raw_mode]. If you don't mess with termios outside of crossterm, it's
/// effectively disabling the raw mode and doing nothing else.
#[cfg(feature = "libc")]
pub(crate) fn disable_raw_mode() -> io::Result<()> {
    let mut original_mode = TERMINAL_MODE_PRIOR_RAW_MODE.lock();
    if let Some(original_mode_ios) = original_mode.as_ref() {
        let tty = tty_fd()?;
        set_terminal_attr(tty.raw_fd(), original_mode_ios)?;
        // Keep it last - remove the original mode only if we were able to switch back
        *original_mode = None;
    }
    Ok(())
}

#[cfg(not(feature = "libc"))]
pub(crate) fn disable_raw_mode() -> io::Result<()> {
    let mut original_mode = TERMINAL_MODE_PRIOR_RAW_MODE.lock();
    if let Some(original_mode_ios) = original_mode.as_ref() {
        let tty = tty_fd()?;
        set_terminal_attr(&tty, original_mode_ios)?;
        // Keep it last - remove the original mode only if we were able to switch back
        *original_mode = None;
    }
    Ok(())
}

#[cfg(not(feature = "libc"))]
fn get_terminal_attr(fd: impl AsFd) -> io::Result<Termios> {
    let result = rustix::termios::tcgetattr(fd)?;
    Ok(result)
}

#[cfg(not(feature = "libc"))]
fn set_terminal_attr(fd: impl AsFd, termios: &Termios) -> io::Result<()> {
    rustix::termios::tcsetattr(fd, rustix::termios::OptionalActions::Now, termios)?;
    Ok(())
}

/// Queries the terminal's support for progressive keyboard enhancement.
///
/// On unix systems, this function will block and possibly time out while
/// [`crossterm::event::read`](crate::event::read) or [`crossterm::event::poll`](crate::event::poll) are being called.
#[cfg(feature = "events")]
pub fn supports_keyboard_enhancement() -> io::Result<bool> {
    query_keyboard_enhancement_flags().map(|flags| flags.is_some())
}

/// Queries the terminal's currently active keyboard enhancement flags.
#[cfg(feature = "events")]
pub fn query_keyboard_enhancement_flags() -> io::Result<Option<KeyboardEnhancementFlags>> {
    query_keyboard_enhancement_flags_with_timeout(std::time::Duration::from_secs(2))
}

/// Queries the terminal's currently active keyboard enhancement flags.
///
/// The event-reader lock and all response waits share `timeout`. A primary
/// device-attributes response without keyboard flags means unsupported (`None`).
/// No response yields `TimedOut`; flags received without device attributes are
/// returned when the deadline expires. Other input remains queued for `read`.
///
/// Queries are written only to the controlling `/dev/tty`, never stdout. The
/// caller must not run another event reader during negotiation. If necessary,
/// raw mode is enabled temporarily and restored before returning.
#[cfg(feature = "events")]
pub fn query_keyboard_enhancement_flags_with_timeout(
    timeout: std::time::Duration,
) -> io::Result<Option<KeyboardEnhancementFlags>> {
    let started = std::time::Instant::now();
    if is_raw_mode_enabled() {
        query_keyboard_enhancement_flags_raw(started, timeout)
    } else {
        enable_raw_mode()?;
        let flags = query_keyboard_enhancement_flags_raw(started, timeout);
        disable_raw_mode()?;
        flags
    }
}

#[cfg(feature = "events")]
fn query_keyboard_enhancement_flags_raw(
    started: std::time::Instant,
    timeout: std::time::Duration,
) -> io::Result<Option<KeyboardEnhancementFlags>> {
    use crate::event::{
        filter::KeyboardEnhancementFlagsFilter, try_lock_internal_event_reader_for, InternalEvent,
    };
    use std::io::Write;

    let timed_out = || {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "The keyboard enhancement status was not received before the deadline",
        )
    };
    let remaining = timeout.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Err(timed_out());
    }
    // Retain the shared reader lock between poll and read so another consumer
    // cannot remove the matching response and turn read into an unbounded wait.
    let mut reader = try_lock_internal_event_reader_for(remaining).ok_or_else(timed_out)?;
    if started.elapsed() >= timeout {
        return Err(timed_out());
    }

    // Query flags followed by primary device attributes, as recommended by
    // https://sw.kovidgoyal.net/kitty/keyboard-protocol/#detection-of-support-for-this-protocol
    // stdout may be an application protocol pipe, so it is never a fallback.
    let mut tty = std::fs::OpenOptions::new().write(true).open("/dev/tty")?;
    tty.write_all(b"\x1B[?u\x1B[c")?;
    tty.flush()?;

    let mut flags = None;
    loop {
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero()
            || !reader.poll(Some(remaining), &KeyboardEnhancementFlagsFilter)?
        {
            return flags.map(Some).ok_or_else(timed_out);
        }
        // The matching event is already queued and the reader is still locked.
        match reader.read(&KeyboardEnhancementFlagsFilter)? {
            InternalEvent::KeyboardEnhancementFlags(current_flags) => flags = Some(current_flags),
            InternalEvent::PrimaryDeviceAttributes => return Ok(flags),
            _ => unreachable!("keyboard response filter accepted an unrelated event"),
        }
    }
}

/// execute tput with the given argument and parse
/// the output as a u16.
///
/// The arg should be "cols" or "lines"
fn tput_value(arg: &str) -> Option<u16> {
    let output = process::Command::new("tput").arg(arg).output().ok()?;
    let value = output
        .stdout
        .into_iter()
        .filter_map(|b| char::from(b).to_digit(10))
        .fold(0, |v, n| v * 10 + n as u16);

    if value > 0 {
        Some(value)
    } else {
        None
    }
}

/// Returns the size of the screen as determined by tput.
///
/// This alternate way of computing the size is useful
/// when in a subshell.
fn tput_size() -> Option<(u16, u16)> {
    match (tput_value("cols"), tput_value("lines")) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    }
}

#[cfg(feature = "libc")]
// Transform the given mode into an raw mode (non-canonical) mode.
fn raw_terminal_attr(termios: &mut Termios) {
    unsafe { cfmakeraw(termios) }
}

#[cfg(feature = "libc")]
fn get_terminal_attr(fd: RawFd) -> io::Result<Termios> {
    unsafe {
        let mut termios = mem::zeroed();
        wrap_with_result(tcgetattr(fd, &mut termios))?;
        Ok(termios)
    }
}

#[cfg(feature = "libc")]
fn set_terminal_attr(fd: RawFd, termios: &Termios) -> io::Result<()> {
    wrap_with_result(unsafe { tcsetattr(fd, TCSANOW, termios) })
}

#[cfg(feature = "libc")]
fn wrap_with_result(result: i32) -> io::Result<()> {
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
