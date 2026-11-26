//! Decoding the bodies of the sequences the scanner finds: OSC 133 marks, percent-escaped command
//! lines and XTGETTCAP names.

use super::*;

pub(crate) fn parse_mark(body: &[u8]) -> Option<Event> {
    let rest = body.strip_prefix(b"133;")?;
    let (kind, params) = match rest.iter().position(|&b| b == b';') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, &rest[rest.len()..]),
    };
    let pairs = || {
        params.split(|&b| b == b';').filter_map(|kv| {
            let i = kv.iter().position(|&b| b == b'=')?;
            Some((&kv[..i], &kv[i + 1..]))
        })
    };
    match kind {
        b"A" => {
            if pairs().any(|(k, v)| k == b"k" && v != b"i") {
                // A secondary or right-side prompt: the same command goes on.
                return None;
            }
            let click_events = pairs().any(|(k, v)| k == b"click_events" && v == b"1");
            Some(Event::Prompt { click_events })
        }
        b"B" => Some(Event::Input),
        b"C" => {
            let command = pairs().find_map(|(k, v)| match k {
                b"cmdline_url" => Some(command_text(&percent_decode(v))),
                b"cmdline" => Some(command_text(v)),
                _ => None,
            });
            Some(Event::Output {
                command: command.filter(|c| !c.is_empty()),
            })
        }
        b"D" => {
            // Extra `key=value` parameters may follow the status.
            let first = params.split(|&b| b == b';').next().unwrap_or(&[]);
            let code = std::str::from_utf8(first)
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
                .map(|n| n.clamp(0, 255) as u8);
            Some(Event::Done(code))
        }
        _ => None,
    }
}

pub(crate) fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// `%XX` escapes decoded; a broken one (the text was cut short) is kept as it is.
pub(crate) fn percent_decode(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == b'%'
            && let (Some(h), Some(l)) = (
                text.get(i + 1).copied().and_then(hex_digit),
                text.get(i + 2).copied().and_then(hex_digit),
            )
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(text[i]);
        i += 1;
    }
    out
}

/// A command line as one line of text: line breaks become spaces, other control characters go.
pub(crate) fn command_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim()
        .chars()
        .map(|c| if c == '\n' || c == '\t' { ' ' } else { c })
        .filter(|c| !c.is_control())
        .take(COMMAND_SHOWN)
        .collect()
}

/// The names in an XTGETTCAP query (`NAME;NAME`, each hex-encoded); `None` if any is not hex.
pub(crate) fn termcap_names(body: &[u8]) -> Option<Vec<String>> {
    body.split(|&b| b == b';')
        .map(|hex| {
            if hex.len() % 2 != 0 {
                return None;
            }
            let bytes = hex
                .chunks(2)
                .map(|p| Some(hex_digit(p[0])? << 4 | hex_digit(p[1])?))
                .collect::<Option<Vec<u8>>>()?;
            String::from_utf8(bytes).ok()
        })
        .collect()
}
