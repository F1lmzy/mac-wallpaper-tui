use anyhow::Result;
use std::env;
use std::process::Command;
use std::time::Duration;
use tokio::time::timeout;

/// Detects terminal capabilities and supported image protocols
pub struct TerminalDetector;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TerminalProtocol {
    /// Kitty Graphics Protocol (best performance)
    Kitty,
    /// iTerm2 Inline Images Protocol
    Iterm2,
    /// Sixel graphics
    Sixel,
    /// No image support - use ASCII fallback
    None,
}

#[derive(Debug, Clone)]
pub struct TerminalCapabilities {
    pub protocol: TerminalProtocol,
    pub font_size: (u32, u32), // width, height in pixels
    pub terminal_name: String,
}

impl TerminalDetector {
    /// Detect terminal capabilities
    pub async fn detect() -> Result<TerminalCapabilities> {
        // First check environment variables (fast path)
        if let Some(caps) = Self::detect_from_env() {
            return Ok(caps);
        }

        // Try to detect via terminal response
        if let Ok(Some(caps)) = Self::detect_via_csi().await {
            return Ok(caps);
        }

        // Default to no image support
        Ok(TerminalCapabilities {
            protocol: TerminalProtocol::None,
            font_size: (8, 16), // default guess
            terminal_name: "unknown".to_string(),
        })
    }

    /// Detect from environment variables
    fn detect_from_env() -> Option<TerminalCapabilities> {
        let term_program = env::var("TERM_PROGRAM").unwrap_or_default();
        let term = env::var("TERM").unwrap_or_default();

        // Check for tmux/screen first
        if term.contains("screen") || env::var("TMUX").is_ok() {
            // Inside multiplexer - may need special handling
            return Self::detect_multiplexer(&term_program);
        }

        // Direct terminal detection
        let (protocol, name) = match term_program.as_str() {
            "ghostty" | "kitty" => (TerminalProtocol::Kitty, term_program.clone()),
            "iTerm.app" | "WezTerm" | "WarpTerminal" => (TerminalProtocol::Iterm2, term_program.clone()),
            "Apple_Terminal" => (TerminalProtocol::Sixel, "Terminal.app".to_string()),
            _ => {
                // Check TERM for additional clues
                if term.contains("kitty") {
                    (TerminalProtocol::Kitty, "kitty".to_string())
                } else if term.contains("foot") {
                    (TerminalProtocol::Sixel, "foot".to_string())
                } else {
                    return None;
                }
            }
        };

        // Try to get font size from terminal
        let font_size = Self::estimate_font_size(&name);

        Some(TerminalCapabilities {
            protocol,
            font_size,
            terminal_name: name,
        })
    }

    /// Detect when running inside a multiplexer
    fn detect_multiplexer(real_term: &str) -> Option<TerminalCapabilities> {
        // Try to find the real terminal
        let outer_term = env::var("TERM_PROGRAM").unwrap_or_default();
        
        let (protocol, name) = match outer_term.as_str() {
            "ghostty" | "kitty" => (TerminalProtocol::Kitty, outer_term),
            "iTerm.app" | "WezTerm" => (TerminalProtocol::Iterm2, outer_term),
            _ => {
                // Check for KITTY_WINDOW_ID which indicates kitty
                if env::var("KITTY_WINDOW_ID").is_ok() {
                    (TerminalProtocol::Kitty, "kitty".to_string())
                } else {
                    (TerminalProtocol::Sixel, "tmux".to_string())
                }
            }
        };

        let font_size = Self::estimate_font_size(&name);

        Some(TerminalCapabilities {
            protocol,
            font_size,
            terminal_name: format!("{} (via multiplexer)", name),
        })
    }

    /// Try to detect via CSI queries
    async fn detect_via_csi() -> Result<Option<TerminalCapabilities>> {
        // This would send CSI sequences and read responses
        // For now, skip this to avoid complexity
        Ok(None)
    }

    /// Estimate font size based on terminal
    fn estimate_font_size(terminal: &str) -> (u32, u32) {
        match terminal {
            "ghostty" => (8, 16),
            "kitty" => (8, 16),
            "iTerm.app" => (7, 14),
            "WezTerm" => (8, 16),
            "foot" => (9, 18),
            _ => (8, 16), // default
        }
    }
}
