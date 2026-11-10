//! The spike's palette: uQ's tokens.json if present, else a built-in Gruvbox dark.
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct Palette {
    pub fg: u32,
    pub bg: u32,
    pub cursor: u32,
    pub accent: u32,
    pub ansi: [u32; 16],
    pub font: String,
    pub font_size: u16,
    pub radius: u16,
    pub border: u16,
}

fn hex(v: &Value) -> Option<u32> {
    let s = v.as_str()?.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    u32::from_str_radix(s, 16).ok()
}

impl Palette {
    pub fn builtin() -> Palette {
        Palette {
            fg: 0xebdbb2,
            bg: 0x282828,
            cursor: 0xebdbb2,
            accent: 0xd79921,
            ansi: [
                0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0xa89984, 0x928374, 0xfb4934,
                0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c, 0xebdbb2,
            ],
            font: "JetBrains Mono".into(),
            font_size: 11,
            radius: 10,
            border: 2,
        }
    }

    pub fn from_tokens(json: &str) -> Option<Palette> {
        let v: Value = serde_json::from_str(json).ok()?;
        let mut ansi = [0u32; 16];
        let list = v["terminal"]["ansi"].as_array()?;
        if list.len() != 16 {
            return None;
        }
        for (slot, item) in ansi.iter_mut().zip(list) {
            *slot = hex(item)?;
        }
        let fg = hex(&v["colors"]["text"])?;
        Some(Palette {
            fg,
            bg: hex(&v["colors"]["bg"])?,
            cursor: fg,
            accent: hex(&v["colors"]["accent"])?,
            ansi,
            font: v["fonts"]["mono"].as_str()?.to_string(),
            font_size: u16::try_from(v["fonts"]["size"].as_u64()?).ok()?,
            radius: u16::try_from(v["ui"]["radius"].as_u64()?).ok()?,
            border: u16::try_from(v["ui"]["border_width"].as_u64()?).ok()?,
        })
    }

    pub fn load() -> Palette {
        let path = std::env::var_os("XDG_STATE_HOME")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))
            .map(|p| p.join("uq/theme/tokens.json"));
        path.and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| Palette::from_tokens(&s))
            .unwrap_or_else(Palette::builtin)
    }
}

#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
