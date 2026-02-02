# mac-wallpaper-tui

A terminal-based wallpaper manager for macOS with live image previews.

## Features

- 📁 Browse directories with image files
- 🖼️ Live image preview in terminal (supports Kitty, Sixel, iTerm2 protocols)
- ⭐ Favorites system with persistence
- 🎲 Random wallpaper selection
- 📜 Recent wallpapers history
- 💾 SQLite database for persistence
- ⚡ Async image loading (non-blocking UI)
- ⌨️ Vim-style keyboard navigation

## Installation

```bash
cargo build --release
```

The binary will be at `./target/release/mac-wallpaper-tui`

## Usage

```bash
# Run the app - it will always open in the configured root directory
./target/release/mac-wallpaper-tui
```

The app is sandboxed to a single root wallpaper directory (default: `/System/Library/Desktop Pictures`). You can navigate subdirectories within the root, but cannot go above it.

If the system wallpapers aren't available, it will automatically fall back to `~/Pictures` or the current directory.

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `j` / `↓` | Navigate down |
| `k` / `↑` | Navigate up |
| `l` / `→` | Enter directory |
| `h` / `←` | Go back |
| `f` | Toggle favorite |
| `r` | Set random wallpaper |
| `R` | Show recent wallpapers |
| `Enter` | Set selected as wallpaper |
| `q` | Quit |

## Configuration

Config file location: `~/.config/mac-wallpaper-tui/config.toml`

Default configuration:
```toml
root_directory = "/System/Library/Desktop Pictures"
show_hidden = false
preview_size = 400
```

The app is restricted to the `root_directory` and its subdirectories. You cannot navigate outside of this directory.

## Data Storage

- **Favorites**: Stored in SQLite at `~/Library/Application Support/mac-wallpaper-tui/app.db`
- **Recent wallpapers**: Last 50 wallpapers, persisted in database
- **Config**: TOML file at `~/.config/mac-wallpaper-tui/config.toml`

## Terminal Support

The app auto-detects and uses the best available image protocol:
- **Kitty graphics protocol** (Ghostty, Kitty) - Best quality
- **Sixel** (Alacritty with patch, xterm) - Good quality
- **Half-blocks** (Any terminal) - Universal fallback

## Requirements

- macOS (uses `osascript` to set wallpapers)
- Terminal with image support recommended (Ghostty, iTerm2, Kitty)
- Rust 1.70+ (for building)

## License

MIT
