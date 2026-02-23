/// Returns true for insert-family modes (`i*`).
pub fn is_insert_like_mode(mode: &str) -> bool {
    mode.starts_with('i')
}

/// Returns true for replace-family modes (`R*`).
pub fn is_replace_like_mode(mode: &str) -> bool {
    mode.starts_with('R')
}

/// Returns true for command-line modes (`c*`).
pub fn is_cmdline_mode(mode: &str) -> bool {
    mode.starts_with('c')
}

/// Returns true for terminal-family modes (`t*`, `nt*`).
pub fn is_terminal_like_mode(mode: &str) -> bool {
    mode.starts_with('t') || mode.starts_with("nt")
}

/// Returns true for visual-family modes (`v`, `V`, block-visual).
pub fn is_visual_like_mode(mode: &str) -> bool {
    matches!(mode.as_bytes().first(), Some(b'v' | b'V' | b'\x16'))
}

#[cfg(test)]
mod tests {
    use super::{
        is_cmdline_mode, is_insert_like_mode, is_replace_like_mode, is_terminal_like_mode,
        is_visual_like_mode,
    };

    #[test]
    fn detects_insert_like_modes() {
        assert!(is_insert_like_mode("i"));
        assert!(is_insert_like_mode("ic"));
        assert!(is_insert_like_mode("ix"));
        assert!(!is_insert_like_mode("n"));
    }

    #[test]
    fn detects_replace_like_modes() {
        assert!(is_replace_like_mode("R"));
        assert!(is_replace_like_mode("Rc"));
        assert!(is_replace_like_mode("Rx"));
        assert!(!is_replace_like_mode("r"));
    }

    #[test]
    fn detects_cmdline_modes() {
        assert!(is_cmdline_mode("c"));
        assert!(is_cmdline_mode("cv"));
        assert!(is_cmdline_mode("ce"));
        assert!(!is_cmdline_mode("n"));
    }

    #[test]
    fn detects_terminal_modes() {
        assert!(is_terminal_like_mode("t"));
        assert!(is_terminal_like_mode("nt"));
        assert!(is_terminal_like_mode("ntT"));
        assert!(!is_terminal_like_mode("n"));
        assert!(!is_terminal_like_mode("i"));
    }

    #[test]
    fn detects_visual_modes() {
        assert!(is_visual_like_mode("v"));
        assert!(is_visual_like_mode("V"));
        assert!(is_visual_like_mode("\u{16}"));
        assert!(!is_visual_like_mode("n"));
    }
}
