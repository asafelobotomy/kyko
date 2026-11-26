use super::*;

#[test]
fn rgb() {
    assert_eq!(parse_rgb("#282828"), Some(0x282828));
    assert_eq!(parse_rgb("#AbCdEf"), Some(0xabcdef));
    for bad in [
        "282828",
        "#282",
        "#2828281",
        "#28282g",
        "red",
        "#28282880",
        "",
        "#",
    ] {
        assert_eq!(parse_rgb(bad), None, "{bad}");
    }
}

#[test]
fn fourteen_sorted_builtins() {
    let keys: Vec<_> = builtin_keys().collect();
    assert_eq!(keys.len(), 14);
    assert!(keys.windows(2).all(|w| w[0] < w[1]));
    assert!(keys.contains(&FALLBACK));
    assert_eq!(builtin("nope"), None);
}
