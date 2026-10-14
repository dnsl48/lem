//! Starts Lem, its terminal display and the OS agent, and connects them.
//!
//! The three are siblings under this process, not parent and child:
//!
//! ```text
//!                      lem-curses-launcher
//!                     /         |          \
//!   lem-curses-lisp  ─ stdio ─►  lem-curses     lem-os-agent
//!   (lem.display.v1)  ◄─ stdio ─  (owns /dev/tty) (lem.os.v1, on a
//!         └────────────── local socket ─────────► socket it was named)
//! ```
//!
//! None of them knows how the others were found or started. Everything
//! about the operating system — where the programs live, unpacking the
//! bundled ones, where logs and sockets go, what an exit means — belongs
//! here. The agent is optional: a session without one runs as before.

mod agent;
#[cfg(feature = "bundle")]
mod bundle;

mod session;

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus};
use std::time::Duration;

use anyhow::{Context, Result};

/// How many sessions' logs the state directory (`session::state_dir`)
/// keeps: each session writes its own pair (`session::log_names`), and the
/// oldest are removed as new ones start. Lem's stderr is never inherited —
/// a backtrace would land on the display and shred it — so it always goes
/// to a file when one can be arranged, same as the agent's.
const KEEP_SESSIONS: usize = 10;

/// How long Lem gets to exit on its own once the display has gone. It
/// normally exits first — its hanging up is what ends the display — so
/// this only runs out when the display ended the session itself (headless,
/// after one frame) or died underneath it.
const GRACE: Duration = Duration::from_millis(500);

/// The programs to run.
struct Programs {
    lisp: PathBuf,
    terminal: PathBuf,
    /// The OS agent, when there is one. Optional: without it the session
    /// runs as before, and the clipboard is the display's.
    agent: Option<PathBuf>,
}

impl Programs {
    /// Direct CLI commands need only the image, without resolving or
    /// unpacking either of the other children.
    fn locate_lisp() -> Result<PathBuf> {
        match env_path("LEM_CURSES_LISP") {
            Some(path) => Ok(path),
            #[cfg(feature = "bundle")]
            None => bundle::lisp(),
            #[cfg(not(feature = "bundle"))]
            None => anyhow::bail!(
                "no Lisp image: set LEM_CURSES_LISP \
                 (`make lisp` in frontends/curses builds dist/lem-curses-lisp)"
            ),
        }
    }

    /// Each comes from its environment variable when set, then from the
    /// bundle when there is one. Otherwise the display binary is looked
    /// for beside this one, which is where Cargo builds it; the Lisp image
    /// has no such home and must be named.
    fn locate() -> Result<Self> {
        let lisp = Self::locate_lisp()?;
        let terminal = match env_path("LEM_CURSES_TERMINAL") {
            Some(path) => path,
            #[cfg(feature = "bundle")]
            None => bundle::terminal()?,
            #[cfg(not(feature = "bundle"))]
            None => std::env::current_exe()
                .context("locating this executable")?
                .with_file_name(format!("lem-curses{}", std::env::consts::EXE_SUFFIX)),
        };
        let agent = match env_path("LEM_CURSES_OS_AGENT") {
            Some(path) => Some(path),
            #[cfg(feature = "bundle")]
            None => Some(bundle::os_agent()?),
            #[cfg(not(feature = "bundle"))]
            None => Some(
                std::env::current_exe()
                    .context("locating this executable")?
                    .with_file_name(format!("lem-os-agent{}", std::env::consts::EXE_SUFFIX)),
            )
            .filter(|path| path.is_file()),
        };
        Ok(Self {
            lisp,
            terminal,
            agent,
        })
    }
}

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// Match `lem:parse-args` when deciding whether to start a display. It
/// scans options even after filenames and treats `--` as a filename,
/// while option operands (even ones spelled `--help`) are consumed.
/// Help, version and missing required operands go directly to the image.
fn direct_cli(args: &[OsString]) -> bool {
    let mut args = args.iter();
    let mut requested = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help" | "-v" | "--version") => requested = true,
            Some("-i" | "--interface" | "--log-filename") => {
                if args.next().is_none() {
                    return true;
                }
            }
            // Lem accepts a missing eval operand, unlike the options above.
            Some("-e" | "--eval") => {
                args.next();
            }
            _ => {}
        }
    }
    requested
}

/// SBCL's saved executable must leave all user options to Lem itself.
fn lisp_args(args: &[OsString]) -> impl Iterator<Item = &OsStr> {
    std::iter::once(OsStr::new("--end-runtime-options")).chain(args.iter().map(OsString::as_os_str))
}

/// No session, pipes or auxiliary children: Lem owns CLI output and errors.
fn run_direct_cli(lisp: &Path, args: &[OsString]) -> Result<ExitCode> {
    let status = duct::cmd(lisp, lisp_args(args))
        // A nested invocation does not belong to its caller's session.
        .env_remove("LEM_SESSION_ID")
        .unchecked()
        .run()
        .with_context(|| format!("running {}", lisp.display()))?
        .status;
    Ok(if status.success() {
        ExitCode::SUCCESS
    } else {
        code(status)
    })
}

/// How the session ended.
struct Ended {
    display: ExitStatus,
    /// `None` when Lem outlived the display and was killed.
    lem: Option<ExitStatus>,
    /// What went wrong without ending the session, reported after it.
    warnings: Vec<String>,
    /// Where Lem's own log went, when it went anywhere.
    lem_log: Option<PathBuf>,
}

/// The OS agent while the session runs, and where its socket lives.
struct RunningAgent {
    agent: agent::Agent,
    dir: PathBuf,
    socket: PathBuf,
    /// Where its log went, when it went anywhere — for the warnings
    /// `stop_agent` reports, not for removal: the agent removes its own.
    log: Option<PathBuf>,
}

/// The directory logs live in for this run, reused across sessions. Not
/// having one is a warning, not a fatal error: both Lem and the agent
/// then run with their stderr discarded rather than inherited, which
/// would let a backtrace land on the display and shred it.
fn resolve_log_dir(warnings: &mut Vec<String>) -> Option<PathBuf> {
    let dir = session::state_dir(std::env::var_os("XDG_STATE_HOME"), std::env::var_os("HOME"));
    match dir {
        Some(dir) => match session::ensure_state_dir(&dir) {
            Ok(()) => Some(dir),
            Err(error) => {
                warnings.push(format!(
                    "no log file: {}: {error}; the OS agent's stderr and Lem's are discarded",
                    dir.display()
                ));
                None
            }
        },
        None => {
            warnings.push(
                "no log file: neither XDG_STATE_HOME nor HOME is set; \
                 the OS agent's stderr and Lem's are discarded"
                    .to_string(),
            );
            None
        }
    }
}

/// Create `path` for a process's stderr, or warn and return `None` so the
/// caller discards it instead — never fatal, same as `resolve_log_dir`'s
/// own no-directory case: a log file that cannot be created (e.g. left
/// root-owned by one `sudo -E` run) must not stop the process it is for.
/// `who` names that process in the warning, e.g. "Lem" or "the OS agent".
fn open_log_or_warn(path: &Path, who: &str, warnings: &mut Vec<String>) -> Option<File> {
    match File::create(path) {
        Ok(file) => Some(file),
        Err(error) => {
            warnings.push(format!(
                "no log file for {who}: {}: {error}; its stderr is discarded",
                path.display()
            ));
            None
        }
    }
}

/// Start the OS agent in a new directory private to this session. Any
/// failure is a warning: Lem runs without an agent. If the launcher
/// itself is killed outright, nothing here runs to clean up afterwards;
/// accepted, since no process can run cleanup code after being killed
/// like that — but the agent's own lifeline-driven cleanup still removes
/// its socket and (being by then empty) this directory, regardless of
/// why the launcher is gone (`agent.rs`'s module doc).
fn start_agent(
    program: &Path,
    session: &str,
    log: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Option<RunningAgent> {
    let dir = session::runtime_dir(
        std::env::var_os("XDG_RUNTIME_DIR"),
        &std::env::temp_dir(),
        session,
    );
    if let Err(error) = session::create_private(&dir) {
        warnings.push(format!("no OS agent: creating {}: {error}", dir.display()));
        return None;
    }
    let socket = dir.join("os-agent.sock");
    match agent::Agent::start(program, &socket, log, session, warnings) {
        Ok(agent) => Some(RunningAgent {
            agent,
            dir,
            socket,
            log: log.map(Path::to_path_buf),
        }),
        Err(error) => {
            let _ = std::fs::remove_dir_all(&dir);
            warnings.push(format!("no OS agent: {error:#}"));
            None
        }
    }
}

/// Stop the agent and remove its directory, noting anything abnormal.
fn stop_agent(running: RunningAgent, warnings: &mut Vec<String>) {
    let see = match &running.log {
        Some(log) => format!("; see {}", log.display()),
        None => String::new(),
    };
    match running.agent.stop(GRACE) {
        Ok(Some(status)) if !status.success() => {
            warnings.push(format!("the OS agent exited with {status}{see}"));
        }
        Ok(Some(_)) => {}
        Ok(None) => warnings.push(format!("the OS agent did not exit and was killed{see}")),
        Err(error) => warnings.push(format!("stopping the OS agent: {error:#}")),
    }
    let _ = std::fs::remove_dir_all(&running.dir);
}

/// The two pipes connecting Lem and the display: Lem is started with one
/// end of each, the display with the other.
struct Pipes {
    lem_stdin: os_pipe::PipeReader,
    lem_stdout: os_pipe::PipeWriter,
    from_lem: os_pipe::PipeReader,
    to_lem: os_pipe::PipeWriter,
}

/// Start Lem and the display, and wait for both to end. Called only
/// after the agent (if any) has started; `run` always stops it — whether
/// this succeeds or not — before deciding what to do with the result.
fn run_both(
    programs: &Programs,
    args: &[OsString],
    session: &str,
    agent: Option<&RunningAgent>,
    lem_log: Option<&Path>,
    pipes: Pipes,
    warnings: &mut Vec<String>,
) -> Result<(ExitStatus, Option<ExitStatus>)> {
    let mut lem = duct::cmd(&programs.lisp, lisp_args(args)).env("LEM_SESSION_ID", session);
    // Lem must see the socket of *this* session's agent, and none at all
    // when there is none — never one inherited from the launcher's own
    // environment (e.g. a nested `lem-curses`, started from inside a
    // Lem whose session has an agent, must not connect to that outer
    // one).
    if let Some(running) = agent {
        lem = lem.env("LEM_OS_AGENT_SOCKET", &running.socket);
    } else {
        lem = lem.env_remove("LEM_OS_AGENT_SOCKET");
    }
    // `stdin_file`/`stdout_file` take the pipe ends by value, so the whole
    // chain through `start` must be one expression, not bound partway:
    // a bound `Expression` lives to the end of this function (Rust drops
    // locals by scope, not by last use), and so would keep its end of the
    // pipe open long after Lem itself has died, hiding that from the
    // display as EOF.
    let lem_start = match lem_log.and_then(|path| open_log_or_warn(path, "Lem", warnings)) {
        Some(stderr) => lem
            .stdin_file(pipes.lem_stdin)
            .stdout_file(pipes.lem_stdout)
            .stderr_file(stderr)
            .unchecked()
            .start(),
        None => lem
            .stdin_file(pipes.lem_stdin)
            .stdout_file(pipes.lem_stdout)
            .stderr_null()
            .unchecked()
            .start(),
    };
    let lem = lem_start.with_context(|| format!("starting {}", programs.lisp.display()))?;

    let display = duct::cmd(&programs.terminal, None::<OsString>)
        .env("LEM_SESSION_ID", session)
        .stdin_file(pipes.from_lem)
        .stdout_file(pipes.to_lem)
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
    Ok((display, lem))
}

/// Run both halves until the display exits, passing `args` to Lem.
fn run(programs: &Programs, args: &[OsString]) -> Result<Ended> {
    let session = session::new_id();
    let mut warnings = Vec::new();

    let log_dir = resolve_log_dir(&mut warnings);
    if let Some(dir) = &log_dir {
        // Room for this session's pair among the ones kept.
        session::prune_logs(dir, KEEP_SESSIONS - 1);
    }
    let (lem_log_name, agent_log_name) = session::log_names(&session);
    let lem_log = log_dir.as_deref().map(|dir| dir.join(&lem_log_name));
    let agent_log = log_dir.as_deref().map(|dir| dir.join(&agent_log_name));

    // One pipe each way. Lem is started with its ends and the display with
    // the others; the expressions holding them are dropped as soon as each
    // has started, so this process keeps no end open, and either half
    // exiting is seen by the other as EOF. Created before the agent starts,
    // so a failure here has nothing to stop yet.
    let (from_lem, lem_stdout) = os_pipe::pipe().context("creating a pipe")?;
    let (lem_stdin, to_lem) = os_pipe::pipe().context("creating a pipe")?;
    let pipes = Pipes {
        lem_stdin,
        lem_stdout,
        from_lem,
        to_lem,
    };

    let agent = programs
        .agent
        .as_deref()
        .and_then(|program| start_agent(program, &session, agent_log.as_deref(), &mut warnings));

    // Whatever run_both returns, the agent (if any) is stopped and its
    // directory removed before this propagates it.
    let outcome = run_both(
        programs,
        args,
        &session,
        agent.as_ref(),
        lem_log.as_deref(),
        pipes,
        &mut warnings,
    );
    if let Some(running) = agent {
        stop_agent(running, &mut warnings);
    }
    let (display, lem) = outcome?;
    Ok(Ended {
        display,
        lem,
        warnings,
        lem_log,
    })
}

/// Report anything abnormal, after both halves are gone — and so after the
/// display has handed the terminal back — and choose the exit code.
fn report(ended: &Ended) -> ExitCode {
    for warning in &ended.warnings {
        eprintln!("lem-curses: {warning}");
    }
    // A fatal editor report also fails the display. Prefer the originating
    // Lem exit and its log; signals still fall behind a display failure.
    if let Some(lem) = ended
        .lem
        .filter(|lem| lem.code().is_some_and(|code| code != 0))
    {
        return report_lem_failure(lem, ended.lem_log.as_deref());
    }
    if !ended.display.success() {
        eprintln!("lem-curses: the display exited with {}", ended.display);
        return code(ended.display);
    }
    match ended.lem {
        Some(lem) if !lem.success() => report_lem_failure(lem, ended.lem_log.as_deref()),
        // Killed because the display ended the session, which it did
        // successfully.
        Some(_) | None => ExitCode::SUCCESS,
    }
}

fn report_lem_failure(lem: ExitStatus, log: Option<&Path>) -> ExitCode {
    match log {
        Some(log) => eprintln!("lem-curses: Lem exited with {lem}; see {}", log.display()),
        None => eprintln!("lem-curses: Lem exited with {lem}"),
    }
    code(lem)
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
    // The command line belongs to Lem: files to open, `--eval`, ...
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if direct_cli(&args) {
        return run_direct_cli(&Programs::locate_lisp()?, &args);
    }
    let programs = Programs::locate()?;
    let ended = run(&programs, &args)?;
    Ok(report(&ended))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn open_log_or_warn_creates_the_file_when_it_can() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        let mut warnings = Vec::new();
        assert!(open_log_or_warn(&log, "Lem", &mut warnings).is_some());
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(log.exists());
    }

    #[test]
    fn open_log_or_warn_falls_back_when_it_cannot_create_the_file() {
        let dir = tempfile::tempdir().unwrap();
        // Its parent is a regular file, so the log can never be created
        // (ENOTDIR) — root-proof, unlike a 0500 directory.
        let not_a_dir = dir.path().join("not-a-dir");
        std::fs::write(&not_a_dir, "").unwrap();
        let log = not_a_dir.join("log");
        let mut warnings = Vec::new();
        assert!(open_log_or_warn(&log, "Lem", &mut warnings).is_none());
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("Lem"), "{warnings:?}");
        assert!(
            warnings[0].contains(&log.display().to_string()),
            "{warnings:?}"
        );
    }
}
