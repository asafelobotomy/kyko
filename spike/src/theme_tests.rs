use super::*;

#[test]
fn builtin_is_gruvbox_dark() {
    let p = Palette::builtin();
    assert_eq!((p.bg, p.fg, p.accent), (0x282828, 0xebdbb2, 0xd79921));
    assert_eq!(p.ansi[1], 0xcc241d);
}

#[test]
fn tokens_parse_and_bad_json_is_none() {
    let j = r##"{"colors":{"bg":"#101010","text":"#eeeeee","accent":"#ff0000"},
      "ui":{"radius":4,"border_width":1},"fonts":{"mono":"Hack","size":12},
      "terminal":{"ansi":["#000000","#111111","#222222","#333333","#444444","#555555","#666666","#777777",
                          "#888888","#999999","#aaaaaa","#bbbbbb","#cccccc","#dddddd","#eeeeee","#ffffff"]}}"##;
    let p = Palette::from_tokens(j).unwrap();
    assert_eq!((p.bg, p.font.as_str(), p.font_size, p.ansi[15]), (0x101010, "Hack", 12, 0xffffff));
    assert!(Palette::from_tokens("{").is_none());
}
