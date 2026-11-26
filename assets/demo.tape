# Terminal demo: golangci-lint waits → guff finishes.
# Timings: helm cold-cache (SCOREBOARD.md).
# Regenerate from repo root: vhs assets/demo.tape

Output assets/demo.gif

Set Shell "bash"
Set FontFamily "JetBrains Mono, Menlo, Monaco, DejaVu Sans Mono, monospace"
Set FontSize 22
Set Width 820
Set Height 320
Set Padding 28
Set Theme "Catppuccin Mocha"
Set WindowBar Colorful
Set WindowBarSize 36
Set BorderRadius 10
Set Framerate 30
Set PlaybackSpeed 1.0
Set TypingSpeed 42ms

Hide
Type `cd "$(git rev-parse --show-toplevel)" && PS1='$ ' && source assets/demo-funcs.sh && clear`
Enter
Show

Type `golangci-lint run ./...`
Enter
Sleep 4.2s

Type `guff run ./...`
Enter
Sleep 2.8s
