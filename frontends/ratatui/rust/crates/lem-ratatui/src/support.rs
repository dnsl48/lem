//! What the terminal can draw, decided from how it identifies itself.

/// Whether the terminal draws curly, dotted, dashed and double underlines
/// (`SGR 4:3` and its siblings), rather than ignoring them (ADR 0016).
///
/// Where it is not known to, underlines are drawn straight, which every
/// terminal draws: what Lem looked like before. `LEM_RATATUI_UNDERCURL`
/// (`1` or `0`) settles it either way, for a terminal this list does not
/// know or a multiplexer configured to pass the styles through.
///
/// `env` looks a variable up; `std::env::var` in the display, a table in
/// tests.
pub fn styled_underlines(env: impl Fn(&str) -> Option<String>) -> bool {
    if let Some(forced) = forced(&env, "LEM_RATATUI_UNDERCURL") {
        return forced;
    }
    // VTE (GNOME Terminal, Tilix, Terminator...) since 0.52.
    !multiplexed(&env) && (modern(&env) || vte_since(&env, 5200))
}

/// Whether the terminal makes hyperlinks of OSC 8 (ADR 0018).
///
/// Where it is not known to, links are not sent: text is drawn as before.
/// `LEM_RATATUI_HYPERLINKS` (`1` or `0`) settles it either way; tmux, for
/// one, passes them on from 3.4 when its `hyperlinks` feature is on.
pub fn hyperlinks(env: impl Fn(&str) -> Option<String>) -> bool {
    if let Some(forced) = forced(&env, "LEM_RATATUI_HYPERLINKS") {
        return forced;
    }
    !multiplexed(&env)
        && (modern(&env)
            // VTE since 0.50, Konsole since 20.12, and Windows Terminal.
            || vte_since(&env, 5000)
            || env("KONSOLE_VERSION")
                .and_then(|version| version.parse::<u32>().ok())
                .is_some_and(|version| version >= 201200)
            || env("WT_SESSION").is_some())
}

/// An override's answer, when the variable is set.
fn forced(env: &impl Fn(&str) -> Option<String>, name: &str) -> Option<bool> {
    env(name).map(|value| matches!(value.as_str(), "1" | "true" | "yes"))
}

/// tmux and screen pass these on only when configured to, and TERM names
/// them rather than the terminal outside.
fn multiplexed(env: &impl Fn(&str) -> Option<String>) -> bool {
    let term = env("TERM").unwrap_or_default();
    env("TMUX").is_some() || term.starts_with("tmux") || term.starts_with("screen")
}

/// The terminals that draw every underline style and make hyperlinks.
fn modern(env: &impl Fn(&str) -> Option<String>) -> bool {
    const TERMS: [&str; 6] = [
        "kitty",
        "wezterm",
        "foot",
        "alacritty",
        "ghostty",
        "contour",
    ];
    let term = env("TERM").unwrap_or_default();
    TERMS.iter().any(|name| term.contains(name))
        || matches!(
            env("TERM_PROGRAM").as_deref(),
            Some("WezTerm" | "ghostty" | "iTerm.app")
        )
}

fn vte_since(env: &impl Fn(&str) -> Option<String>, version: u32) -> bool {
    env("VTE_VERSION")
        .and_then(|found| found.parse::<u32>().ok())
        .is_some_and(|found| found >= version)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    }

    fn with(vars: &[(&str, &str)]) -> bool {
        styled_underlines(env(vars))
    }

    fn links(vars: &[(&str, &str)]) -> bool {
        hyperlinks(env(vars))
    }

    #[test]
    fn terminals_known_to_make_hyperlinks() {
        assert!(links(&[("TERM", "xterm-kitty")]));
        assert!(links(&[
            ("TERM", "xterm-256color"),
            ("VTE_VERSION", "5000")
        ]));
        assert!(links(&[
            ("TERM", "xterm-256color"),
            ("KONSOLE_VERSION", "230804")
        ]));
        assert!(links(&[("TERM", "xterm-256color"), ("WT_SESSION", "x")]));
    }

    #[test]
    fn hyperlinks_elsewhere_only_when_told() {
        assert!(!links(&[("TERM", "xterm-256color")]));
        assert!(!links(&[("TERM", "linux")]));
        assert!(!links(&[
            ("TERM", "xterm-256color"),
            ("KONSOLE_VERSION", "200800")
        ]));
        assert!(!links(&[("TERM", "tmux-256color")]));
        assert!(links(&[
            ("TERM", "tmux-256color"),
            ("LEM_RATATUI_HYPERLINKS", "1")
        ]));
        assert!(!links(&[
            ("TERM", "xterm-kitty"),
            ("LEM_RATATUI_HYPERLINKS", "0")
        ]));
        assert!(
            !links(&[
                ("TERM", "xterm-kitty"),
                ("LEM_RATATUI_UNDERCURL", "1"),
                ("LEM_RATATUI_HYPERLINKS", "0")
            ]),
            "each override settles its own feature"
        );
    }

    #[test]
    fn terminals_known_to_draw_them() {
        assert!(with(&[("TERM", "xterm-kitty")]));
        assert!(with(&[("TERM", "foot")]));
        assert!(with(&[("TERM", "alacritty")]));
        assert!(with(&[
            ("TERM", "xterm-256color"),
            ("TERM_PROGRAM", "WezTerm")
        ]));
        assert!(with(&[("TERM", "xterm-256color"), ("VTE_VERSION", "7600")]));
    }

    #[test]
    fn anything_else_is_straight() {
        assert!(!with(&[("TERM", "xterm-256color")]));
        assert!(!with(&[("TERM", "linux")]));
        assert!(!with(&[]));
        assert!(!with(&[("VTE_VERSION", "5000")]), "VTE before 0.52");
    }

    #[test]
    fn a_multiplexer_is_straight_unless_told() {
        assert!(!with(&[("TERM", "tmux-256color")]));
        assert!(!with(&[
            ("TERM", "xterm-kitty"),
            ("TMUX", "/tmp/tmux-1000/default,1,0")
        ]));
        assert!(with(&[
            ("TERM", "tmux-256color"),
            ("LEM_RATATUI_UNDERCURL", "1")
        ]));
    }

    #[test]
    fn the_override_wins_both_ways() {
        assert!(!with(&[
            ("TERM", "xterm-kitty"),
            ("LEM_RATATUI_UNDERCURL", "0")
        ]));
        assert!(with(&[("TERM", "linux"), ("LEM_RATATUI_UNDERCURL", "1")]));
    }
}
