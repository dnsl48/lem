//! Starts Lem and its terminal display, and connects them.
//!
//! The two halves are siblings under this process, not parent and child:
//!
//! ```text
//!                      lem-ratatui-launcher
//!                     /                    \
//!   lem-ratatui-lisp  ──── stdout → stdin ───►  lem-ratatui
//!   (JSON-RPC, stdio) ◄─── stdin ← stdout ────  (owns /dev/tty)
//! ```
//!
//! Neither half knows how the other was found or started; each only
//! speaks the protocol on its stdio. Everything about the operating
//! system — where the programs live, unpacking the bundled ones, where
//! logs go, what an exit means — belongs here.

#[cfg(feature = "bundle")]
mod bundle;

use std::ffi::OsString;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus};
use std::time::Duration;

use anyhow::{Context, Result};

/// Where Lem's stderr goes. Inherited, a backtrace would land on the
/// display and shred it.
const LOG: &str = "/tmp/lem-ratatui.log";

/// How long Lem gets to exit on its own once the display has gone. It
/// normally exits first — its hanging up is what ends the display — so
/// this only runs out when the display ended the session itself (headless,
/// after one frame) or died underneath it.
const GRACE: Duration = Duration::from_millis(500);

/// The two programs to run.
struct Programs {
    lisp: PathBuf,
    terminal: PathBuf,
}

impl Programs {
    /// Each comes from its environment variable when set, then from the
    /// bundle when there is one. Otherwise the display binary is looked
    /// for beside this one, which is where Cargo builds it; the Lisp image
    /// has no such home and must be named.
    fn locate() -> Result<Self> {
        let lisp = match env_path("LEM_RATATUI_LISP") {
            Some(path) => path,
            #[cfg(feature = "bundle")]
            None => bundle::lisp()?,
            #[cfg(not(feature = "bundle"))]
            None => anyhow::bail!(
                "no Lisp image: set LEM_RATATUI_LISP \
                 (`make lisp` in frontends/ratatui builds dist/lem-ratatui-lisp)"
            ),
        };
        let terminal = match env_path("LEM_RATATUI_TERMINAL") {
            Some(path) => path,
            #[cfg(feature = "bundle")]
            None => bundle::terminal()?,
            #[cfg(not(feature = "bundle"))]
            None => std::env::current_exe()
                .context("locating this executable")?
                .with_file_name(format!("lem-ratatui{}", std::env::consts::EXE_SUFFIX)),
        };
        Ok(Self { lisp, terminal })
    }
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// How the session ended.
struct Ended {
    display: ExitStatus,
    /// `None` when Lem outlived the display and was killed.
    lem: Option<ExitStatus>,
}

/// Run both halves until the display exits, passing `args` to Lem.
fn run(programs: &Programs, args: &[OsString], log: &Path) -> Result<Ended> {
    // One pipe each way. Lem is started with its ends and the display with
    // the others; the expressions holding them are dropped as soon as each
    // has started, so this process keeps no end open, and either half
    // exiting is seen by the other as EOF.
    let (from_lem, lem_stdout) = os_pipe::pipe().context("creating a pipe")?;
    let (lem_stdin, to_lem) = os_pipe::pipe().context("creating a pipe")?;
    let stderr = File::create(log).with_context(|| format!("creating {}", log.display()))?;

    let lem = duct::cmd(&programs.lisp, args)
        .stdin_file(lem_stdin)
        .stdout_file(lem_stdout)
        .stderr_file(stderr)
        .unchecked()
        .start()
        .with_context(|| format!("starting {}", programs.lisp.display()))?;
    let display = duct::cmd(&programs.terminal, None::<OsString>)
        .stdin_file(from_lem)
        .stdout_file(to_lem)
        .unchecked()
        .start();
    let display = match display {
        Ok(display) => display,
        Err(error) => {
            let _ = lem.kill();
            return Err(error).with_context(|| format!("starting {}", programs.terminal.display()));
        }
    };

    let display = display.wait()?.status;
    let lem = match lem.wait_timeout(GRACE)? {
        Some(output) => Some(output.status),
        None => {
            lem.kill()?;
            lem.wait()?;
            None
        }
    };
    Ok(Ended { display, lem })
}

/// Report anything abnormal, after both halves are gone — and so after the
/// display has handed the terminal back — and choose the exit code.
fn report(ended: &Ended, log: &Path) -> ExitCode {
    if !ended.display.success() {
        eprintln!("lem-ratatui: the display exited with {}", ended.display);
        return code(ended.display);
    }
    match ended.lem {
        Some(lem) if !lem.success() => {
            eprintln!("lem-ratatui: Lem exited with {lem}; see {}", log.display());
            code(lem)
        }
        // Killed because the display ended the session, which it did
        // successfully.
        Some(_) | None => ExitCode::SUCCESS,
    }
}

/// The exit code to pass on for `status`, which on Unix has none when a
/// signal ended it.
fn code(status: ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .filter(|&code| code != 0)
        .map_or(ExitCode::FAILURE, ExitCode::from)
}

fn main() -> Result<ExitCode> {
    let programs = Programs::locate()?;
    // The command line belongs to Lem: files to open, `--eval`, ...
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let log = PathBuf::from(LOG);
    let ended = run(&programs, &args, &log)?;
    Ok(report(&ended, &log))
}
