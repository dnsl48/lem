//! Child process for keyboard_probe_pty.py; stdout deliberately is a protocol pipe.
use crossterm::{event, terminal};
use std::time::{Duration, Instant};

fn main() {
    let mut args = std::env::args().skip(1);
    let timeout = Duration::from_millis(args.next().unwrap().parse().unwrap());
    let key_count: usize = args.next().unwrap().parse().unwrap();
    let started = Instant::now();
    println!("RAW_BEFORE={}", terminal::is_raw_mode_enabled().unwrap());
    let result = terminal::query_keyboard_enhancement_flags_with_timeout(timeout);
    match result {
        Ok(Some(flags)) => println!("RESULT=flags:{}", flags.bits()),
        Ok(None) => println!("RESULT=none"),
        Err(error) => println!("RESULT=error:{:?}", error.kind()),
    }
    println!("ELAPSED_MS={}", started.elapsed().as_millis());
    println!("RAW_AFTER={}", terminal::is_raw_mode_enabled().unwrap());
    for _ in 0..key_count {
        assert!(
            event::poll(Duration::from_millis(100)).unwrap(),
            "lost queued key"
        );
        match event::read().unwrap() {
            event::Event::Key(key) => match key.code {
                event::KeyCode::Char(character) => {
                    println!("KEY={}:{}", character as u32, key.modifiers.bits());
                }
                other => panic!("unexpected key: {other:?}"),
            },
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
