# mooagent

```
.....................................................................
......----..:..:-:-----=**++=-+=:----:::..............:.:.........:-.
......----:.:..::-:.::::==*@@@@@@@@@@@@%=.............:.:.......::-=.
...:..----:.:..::--.::::=@@@@@@@@@@@@@@@@#............:.:.......:.--.
..::.:----.:....:--:::-%@@@+::======-=*@@@@-............:.::....::-:.
.----=--:-::--:.::--.+@@@@---:-:==-:=-:-#@@@*-...--=#@@@@@*=--:::---.
.%%#%##%@@@@@@@@@@@@@@@@-=::--.:=:-===.:-=@@@@@@@@@@@@@@@@@@@*=--+*+.
.@@@@@@@#-::+%@@@@@@@@=+=:..:===-:-=-+===+-%@@@@@@@@%=@@@@==@@@@@@@%.
.@@@@@@+@@@@@@@%%@@#+%@*=---:.:-----------==++#@@@@%#%@@@@@@*@@@@@%#.
.%@@@@@@@+=+#-+*%@@@@@@#++**+---:-:-.::-.:---#%@@@@#+++*-=#@@-==----.
.::.:-%@@@@#=:=+%%######+*++**+--:-----===-::#@%%##+===%@@@@=--::---.
..:..:-*@@@@=.-=+-::.=##*@@%*#%*=::-----===#@@%@=-+#:-=+@%=---:::..:.
.----====-%@@@@###%@@#+##*=*++*+=-::====.--@%%=@@%+#+*=*-----:::::.:.
.=+=--::-:-#@@@@%@@@@@+=-%@#+++====--:+=---@@@=##%%#=-==-------:--::.
.-::::--:.-+*++#*+===##+=-=*++-:-===-.=++-:==:-::-=-.--:::.:.:..::--.
.-:-=+++==++==----==-+==-.:::-====-=-:::=---=-:::::::.::-::::::::.::.
.-==++=-====-:--=--=-+:----:::==#---=-.:-=--=::::::-------::...::---.
.----------:::-:-=+=-+::-::---=++-+-:=--:====::::.:::::::.::::::-:::.
.---=:-:::::::-:----=+--:::::::==:#@#*+=++++-::::::.::::--::::::::.:.
.=--=:-:--::----:-::-++--------=:%@@@@%@@@@=--:.::..::..:.::..:...:-.
.-::--:-::---===-::--++-------=+-%%=*@@@%=#+---:--:::::::::.......::.
..::--:-----====+::--+==----:-+=#%**-*#**+#+---:::::.::..::::::.::.:.
.---:.:==:.--=-=*+--+=-==-=+=-=+**++=:-=+#*=---:.--:::::::-:::::::...
..:--:-==-:--=:-==-=+----*=+#*==-+**++*#@@@+:---:-:.:.:....:.::...::.
..-::----:-=-:-++:-++=+==#=-=*#+=:=*#%%@@@=*-:.:-::::::..:-::::::-==.
..:------==-:-==:-+%##*=++--::=+----+#@@%-+*------::....::..::-====+.
.::--:.:=--:--==+=#%++=:---::-:==---=++===#=.:-:::::-----:.:-===++++.
.-----:-=::----==+#*.-=:=-:.::--=--+==++=+#-::.:-::..::..:=++++==+#%.
.--::-==---::-=-==+=::==---.:..-=-=+=-+==**-:.:::--:----=+*++===+**+.
.....................................................................
                                    M O O !
```

TUI for managing AI agent configuration files across multiple agents.

## Usage

```bash
# Run the TUI
cargo run

# Or install it
cargo install --path .
mooagent
```

## Keys

### Navigation
- `Tab` / `Ctrl+w` - Cycle focus between panes (**Agent List**, **Global Rules**, **Project Rules**)
- `h` / `l` or `←` / `→` - Move focus horizontally between rules panes
- `j` / `k` or `↓` / `↑` - Navigate/Scroll within the focused pane
- `gg` / `G` - Jump to Top / Bottom of focused pane
- `Ctrl+u` / `Ctrl+d` - Half-page Up / Down focused pane
- `Mouse Scroll` - Scroll focused pane

### Actions
- `s` - Sync all agent files (with confirmation)
- `Enter` - Sync selected agent (with confirmation)
- `d` - View diff for selected agent
- `b` - View backups for selected agent
- `Ctrl+g` - Edit global rules (syncs to all agent global files)
- `Ctrl+e` - Edit project rules (AGENTS.md)
- `Ctrl+c` - Edit config file (.mooagent.toml)
- `a` - Toggle auto-sync
- `/` - Search agents by name/path
- `v` - Toggle status/error log
- `?` - Show help
- `q` / `Esc` - Quit or close dialog

## Configuration

Create `.mooagent.toml` in your project root:

```toml
[[agents]]
name = "Claude"
path = "CLAUDE.md"
strategy = "merge"                          # copies AGENTS.md content
global_file = "~/.claude/CLAUDE.md"         # optional: where global rules go

[[agents]]
name = "Gemini"
path = "GEMINI.md"
strategy = "symlink"                        # symlinks to AGENTS.md
global_file = "~/.gemini/GEMINI.md"
```

Default agents: Claude, Gemini, OpenCode (all merge strategy).

## Architecture

**Two-layer system:**
1. **Global rules** (`~/.config/mooagent/GLOBAL_RULES.md`)
   - Synced to agent-specific global files when you edit with `g`
   - Each agent reads from their global location
2. **Project rules** (`AGENTS.md` in project root)
   - Synced to project-specific agent files (CLAUDE.md, etc.)
   - Strategy determines sync method (copy vs symlink)

**Backups:** Stored in `~/.local/share/mooagent/backups/`
