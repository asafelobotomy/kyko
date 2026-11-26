#[path = "../common/mod.rs"]
mod common;

use common::{Rng, cases};
use kyko_core::config::*;
use kyko_core::keys::{Key, KeySeq, Mods};
use kyko_core::theme::{CustomColors, ThemeSource};

fn parse(text: &str) -> Loaded {
    Config::parse(text).unwrap_or_else(|e| panic!("{text}\n{e}"))
}

fn seq(s: &str) -> KeySeq {
    s.parse().unwrap()
}

/// Every key set to a valid value that is not its default.
const FULL: &str = r##"
[hotkey]
toggle = "Ctrl+Alt+T"

[window]
shape = "sheet"
position = "left"
tabs = "bottom"
height_pct = 60
width_pct = 70
side_width_pct = 35
side_height_pct = 90
center_width_pct = 50
center_height_pct = 55
dim_pct = 0
opacity_pct = 85
monitor = "DP-1"
above_fullscreen = false
hide_on_focus_loss = false
animations = false
opacity = 0.9

[theme]
source = "custom"

[theme.custom]
base = "dracula-light"
foreground = "#101010"
background = "#202020"
cursor = "#303030"
selection = "#404040"
selection_text = "#505050"
ansi = ["#000001", "#000002"]

[terminal]
font = "Iosevka"
font_size = 13
line_height = 1.2
padding = 4
cursor = "beam"
cursor_blink = true
new_tab_dir = "home"
bell = "sound"
notify_on_finish = "unfocused"
notify_after = 30
copy_on_select = true
ligatures = false
shell_integration = false
shell_extras = false
confirm_multiline_paste = false
scrollback = 50000
shell = "/usr/bin/fish"

[sessions]
ring_kib = 4096

[keys]
new_tab = "Ctrl+Alt+N"
close_pane = "Ctrl+Alt+W"
prev_tab = "Ctrl+Shift+PgUp"
next_tab = "Ctrl+Shift+PgDn"
tab_1 = "Super+1"
tab_9 = "Super+9"
split_side = "Ctrl+Alt+D"
split_stacked = "Ctrl+Alt+E"
focus_left = "Ctrl+Alt+Left"
focus_right = "Ctrl+Alt+Right"
focus_up = "Ctrl+Alt+Up"
focus_down = "Ctrl+Alt+Down"
copy = "Ctrl+Alt+C"
paste = "Ctrl+Alt+V"
search = "Ctrl+Alt+F"
font_bigger = "Ctrl+="
font_smaller = "Ctrl+_"
font_reset = "Ctrl+Alt+0"

[advanced]
programs_write_clipboard = true
allow_run = false
"##;

/// (section, key, a bad value as TOML).
const BAD: &[(&str, &str, &str)] = &[
    ("hotkey", "toggle", "\"Ctrl+Nope\""),
    ("hotkey", "toggle", "12"),
    ("window", "shape", "\"round\""),
    ("window", "shape", "true"),
    ("window", "tabs", "\"left\""),
    ("window", "height_pct", "19"),
    ("window", "height_pct", "101"),
    ("window", "height_pct", "45.0"),
    ("window", "height_pct", "\"45\""),
    ("window", "width_pct", "19"),
    ("window", "position", "\"middle\""),
    ("window", "dim_pct", "81"),
    ("window", "opacity_pct", "29"),
    ("window", "width_pct", "-1"),
    ("window", "monitor", "\"\""),
    ("window", "monitor", "1"),
    ("window", "above_fullscreen", "\"yes\""),
    ("window", "hide_on_focus_loss", "1"),
    ("window", "animations", "[]"),
    ("window", "opacity", "0.49"),
    ("window", "opacity", "1.01"),
    ("window", "opacity", "nan"),
    ("window", "opacity", "inf"),
    ("window", "opacity", "-inf"),
    ("window", "opacity", "\"1\""),
    ("theme", "source", "\"solarized\""),
    ("theme", "source", "\"builtin:\""),
    ("theme", "source", "3"),
    ("terminal", "font", "\"\""),
    ("terminal", "font", "12"),
    ("terminal", "font_size", "5"),
    ("terminal", "font_size", "49"),
    ("terminal", "font_size", "12.5"),
    ("terminal", "line_height", "0.79"),
    ("terminal", "line_height", "2.01"),
    ("terminal", "line_height", "nan"),
    ("terminal", "line_height", "+inf"),
    ("terminal", "padding", "33"),
    ("terminal", "cursor", "\"bar\""),
    ("terminal", "cursor_blink", "0"),
    ("terminal", "new_tab_dir", "\"/tmp\""),
    ("terminal", "bell", "\"loud\""),
    ("terminal", "notify_on_finish", "\"sometimes\""),
    ("terminal", "notify_after", "86401"),
    ("terminal", "copy_on_select", "\"true\""),
    ("terminal", "confirm_multiline_paste", "{}"),
    ("terminal", "scrollback", "100001"),
    ("terminal", "scrollback", "-5"),
    ("terminal", "scrollback", "9223372036854775807"),
    ("terminal", "shell", "\"fish\""),
    ("terminal", "shell", "\"\""),
    ("terminal", "shell", "1"),
    ("sessions", "ring_kib", "255"),
    ("sessions", "ring_kib", "16385"),
    ("keys", "new_tab", "\"Ctrl+Shift+Shift+T\""),
    ("keys", "copy", "7"),
    ("keys", "tab_3", "\"F25\""),
    ("advanced", "allow_run", "\"no\""),
    ("advanced", "programs_write_clipboard", "1979-05-27"),
];

/// `FULL` with `section.key`'s value replaced; `__drop__` removes the line.
fn replace(text: &str, section: &str, key: &str, value: &str) -> Option<String> {
    let mut out = String::new();
    let mut current = String::new();
    let mut found = false;
    for line in text.lines() {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.to_string();
        }
        if current == section && line.starts_with(&format!("{key} = ")) {
            found = true;
            if value != "__drop__" {
                out.push_str(&format!("{key} = {value}\n"));
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    found.then_some(out)
}

mod load;
mod parse;
