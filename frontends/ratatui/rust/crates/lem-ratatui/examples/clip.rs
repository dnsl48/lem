//! Test helper: read or write the system clipboard.
//!
//!     cargo run -p lem-ratatui --example clip -- get
//!     cargo run -p lem-ratatui --example clip -- hold TEXT
//!
//! `hold` blocks, because on X11 the clipboard is served by the process
//! that owns it: a setter that exits takes the contents with it. That is
//! also why text copied out of Lem is gone once Lem exits.

fn main() {
    let mut args = std::env::args().skip(1);
    let mut clipboard = arboard::Clipboard::new().expect("no system clipboard");
    match args.next().as_deref() {
        Some("get") => println!("{}", clipboard.get_text().unwrap_or_default()),
        Some("hold") => {
            let text: Vec<String> = args.collect();
            #[cfg(target_os = "linux")]
            {
                use arboard::SetExtLinux;
                clipboard
                    .set()
                    .wait()
                    .text(text.join(" "))
                    .expect("hold clipboard");
            }
            #[cfg(not(target_os = "linux"))]
            {
                clipboard.set_text(text.join(" ")).expect("set clipboard");
                std::thread::sleep(std::time::Duration::from_secs(3600));
            }
        }
        _ => eprintln!("usage: clip get | clip hold TEXT"),
    }
}
