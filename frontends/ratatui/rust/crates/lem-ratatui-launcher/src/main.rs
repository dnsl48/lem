//! Starts Lem, its terminal display and the OS agent, and connects them.
//!
//! The three are siblings under this process, not parent and child:
//!
//! ```text
//!                      lem-ratatui-launcher
//!                     /         |          \
//!   lem-ratatui-lisp  ─ stdio ─►  lem-ratatui     lem-os-agent
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

use std::ffi::OsString;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{ExitCode, ExitStatus};
use std::time::Duration;

use anyhow::{Context, Result};

/// Where Lem's stderr goes. Inherited, a backtrace would land on the
/// display and shred it.
const LOG: &str = "/tmp/lem-ratatui.log";

/// Where the OS agent's stderr goes.
const AGENT_LOG: &str = "/tmp/lem-ratatui-os-agent.log";

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
        let agent = match env_path("LEM_RATATUI_OS_AGENT") {
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

/// How the session ended.
struct Ended {
    display: ExitStatus,
    /// `None` when Lem outlived the display and was killed.
    lem: Option<ExitStatus>,
    /// What went wrong without ending the session, reported after it.
    warnings: Vec<String>,
}

/// The OS agent while the session runs, and where its socket lives.
struct RunningAgent {
    agent: agent::Agent,
    dir: PathBuf,
    socket: PathBuf,
}

/// Start the OS agent in a new directory private to this session. Any
/// failure is a warning: Lem runs without an agent.
fn start_agent(program: &Path, session: &str, warnings: &mut Vec<String>) -> Option<RunningAgent> {
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
    match agent::Agent::start(program, &socket, Path::new(AGENT_LOG)) {
        Ok(agent) => Some(RunningAgent { agent, dir, socket }),
        Err(error) => {
            let _ = std::fs::remove_dir_all(&dir);
            warnings.push(format!("no OS agent: {error:#}"));
            None
        }
    }
}

/// Stop the agent and remove its directory, noting anything abnormal.
fn stop_agent(running: RunningAgent, warnings: &mut Vec<String>) {
    match running.agent.stop(GRACE) {
        Ok(Some(status)) if !status.success() => {
            warnings.push(format!(
                "the OS agent exited with {status}; see {AGENT_LOG}"
            ));
        }
        Ok(Some(_)) => {}
        Ok(None) => warnings.push(format!(
            "the OS agent did not exit and was killed; see {AGENT_LOG}"
        )),
        Err(error) => warnings.push(format!("stopping the OS agent: {error:#}")),
    }
    let _ = std::fs::remove_dir_all(&running.dir);
}

/// Run both halves until the display exits, passing `args` to Lem.
fn run(programs: &Programs, args: &[OsString], log: &Path) -> Result<Ended> {
    let session = session::new_id();
    let mut warnings = Vec::new();
    let agent = programs
        .agent
        .as_deref()
        .and_then(|program| start_agent(program, &session, &mut warnings));

    // One pipe each way. Lem is started with its ends and the display with
    // the others; the expressions holding them are dropped as soon as each
    // has started, so this process keeps no end open, and either half
    // exiting is seen by the other as EOF.
    let (from_lem, lem_stdout) = os_pipe::pipe().context("creating a pipe")?;
    let (lem_stdin, to_lem) = os_pipe::pipe().context("creating a pipe")?;
    let stderr = File::create(log).with_context(|| format!("creating {}", log.display()))?;

    let mut lem = duct::cmd(&programs.lisp, args).env("LEM_SESSION_ID", &session);
    if let Some(running) = &agent {
        lem = lem.env("LEM_OS_AGENT_SOCKET", &running.socket);
    }
    let lem = lem
        .stdin_file(lem_stdin)
        .stdout_file(lem_stdout)
        .stderr_file(stderr)
        .unchecked()
        .start();
    let lem = match lem {
        Ok(lem) => lem,
        Err(error) => {
            if let Some(running) = agent {
                stop_agent(running, &mut warnings);
            }
            return Err(error).with_context(|| format!("starting {}", programs.lisp.display()));
        }
    };
    let display = duct::cmd(&programs.terminal, None::<OsString>)
        .env("LEM_SESSION_ID", &session)
        .stdin_file(from_lem)
        .stdout_file(to_lem)
        .unchecked()
        .start();
    let display = match display {
        Ok(display) => display,
        Err(error) => {
            let _ = lem.kill();
            if let Some(running) = agent {
                stop_agent(running, &mut warnings);
            }
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
    if let Some(running) = agent {
        stop_agent(running, &mut warnings);
    }
    Ok(Ended {
        display,
        lem,
        warnings,
    })
}

/// Report anything abnormal, after both halves are gone — and so after the
/// display has handed the terminal back — and choose the exit code.
fn report(ended: &Ended, log: &Path) -> ExitCode {
    for warning in &ended.warnings {
        eprintln!("lem-ratatui: {warning}");
    }
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
