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
    if let Some(forced) = env("LEM_RATATUI_UNDERCURL") {
        return matches!(forced.as_str(), "1" | "true" | "yes");
    }
    let term = env("TERM").unwrap_or_default();
    // tmux and screen pass the styles on only when configured to, and
    // TERM names them rather than the terminal outside.
    if env("TMUX").is_some() || term.starts_with("tmux") || term.starts_with("screen") {
        return false;
    }
    const TERMS: [&str; 6] = [
        "kitty",
        "wezterm",
        "foot",
        "alacritty",
        "ghostty",
        "contour",
    ];
    if TERMS.iter().any(|name| term.contains(name)) {
        return true;
    }
    if matches!(
        env("TERM_PROGRAM").as_deref(),
        Some("WezTerm" | "ghostty" | "iTerm.app")
    ) {
        return true;
    }
    // VTE (GNOME Terminal, Tilix, Terminator...) since 0.52.
    env("VTE_VERSION")
        .and_then(|version| version.parse::<u32>().ok())
        .is_some_and(|version| version >= 5200)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(vars: &[(&str, &str)]) -> bool {
        let vars: Vec<(String, String)> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        styled_underlines(|name| vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()))
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
