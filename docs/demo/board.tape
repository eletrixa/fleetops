# fleetops board demo — render with: vhs docs/demo/board.tape
# Produces docs/demo/board.gif (wire it into the README's demo placeholder).
# VHS: https://github.com/charmbracelet/vhs   (`vhs docs/demo/board.tape`)
#
# Run this inside WSL2 with a live fleet of Claude Code sessions for a real board;
# otherwise it records an empty board (still a valid smoke of the UI chrome).

Output docs/demo/board.gif

Set Shell bash
Set FontSize 16
Set Width 1200
Set Height 700
Set Padding 20
Set Theme "Catppuccin Mocha"

# Build once so the recording doesn't capture compile output.
Hide
Type "cargo build --release" Enter
Wait
Show

Type "./target/release/fleet" Enter
Sleep 2s

# Move the selection down the fleet.
Type "j"  Sleep 700ms
Type "j"  Sleep 700ms
Type "k"  Sleep 700ms

# Refresh the board.
Type "r"  Sleep 1.5s

# (Enter would jump to the selected session's wezterm pane.)
Sleep 1s

# Quit.
Type "q"
Sleep 500ms
