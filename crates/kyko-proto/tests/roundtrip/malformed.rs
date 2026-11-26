//! Malformed frames and hellos of other versions: rejected, never a panic.

use super::*;

#[test]
fn inner_length_overrunning_the_frame_is_malformed() {
    let mut body = vec![3u8];
    body.extend(1u32.to_le_bytes());
    body.extend(1000u32.to_le_bytes());
    body.extend([1, 2]);
    let mut d = Decoder::new();
    d.push(&(body.len() as u32).to_le_bytes());
    d.push(&body);
    assert!(matches!(
        d.next::<ToHolder>(),
        Err(ProtoError::Malformed(_))
    ));
}

#[test]
fn huge_claimed_list_count_is_malformed_without_allocating() {
    // Busy { panes: claims u32::MAX entries, holds one }
    let mut body = vec![69u8];
    body.extend(u32::MAX.to_le_bytes());
    body.extend(1u32.to_le_bytes());
    let mut d = Decoder::new();
    d.push(&framed(&body));
    assert!(matches!(
        d.next::<ToWindow>(),
        Err(ProtoError::Malformed(_))
    ));
    // Hello with a huge pane count
    let mut body = vec![65u8];
    body.extend(VERSION.to_le_bytes());
    body.extend(u32::MAX.to_le_bytes());
    let mut d = Decoder::new();
    d.push(&framed(&body));
    assert!(matches!(
        d.next::<ToWindow>(),
        Err(ProtoError::Malformed(_))
    ));
}

#[test]
fn truncated_body_is_malformed() {
    // Close with only 3 of its 4 pane bytes
    let mut d = Decoder::new();
    d.push(&framed(&[5, 1, 0, 0]));
    assert!(matches!(
        d.next::<ToHolder>(),
        Err(ProtoError::Malformed(_))
    ));
}

#[test]
fn trailing_bytes_in_a_frame_are_malformed() {
    let mut body = vec![5u8];
    body.extend(7u32.to_le_bytes());
    body.push(0);
    let mut d = Decoder::new();
    d.push(&framed(&body));
    assert!(matches!(
        d.next::<ToHolder>(),
        Err(ProtoError::Malformed(_))
    ));
}

#[test]
fn invalid_utf8_in_a_string_is_malformed() {
    let mut body = vec![2u8];
    body.extend(1u32.to_le_bytes()); // pane
    body.extend(1u32.to_le_bytes()); // shell length
    body.push(0xff); // shell, invalid UTF-8
    body.extend(0u32.to_le_bytes()); // args
    body.extend(0u32.to_le_bytes()); // cwd
    body.extend(0u32.to_le_bytes()); // env
    body.extend(80u16.to_le_bytes());
    body.extend(24u16.to_le_bytes());
    body.extend(1024u32.to_le_bytes());
    let mut d = Decoder::new();
    d.push(&framed(&body));
    assert!(matches!(
        d.next::<ToHolder>(),
        Err(ProtoError::Malformed(_))
    ));
}

#[test]
fn bad_bool_and_option_flags_are_malformed() {
    let mut body = vec![8u8, 2];
    let mut d = Decoder::new();
    d.push(&framed(&body));
    assert!(matches!(
        d.next::<ToHolder>(),
        Err(ProtoError::Malformed(_))
    ));
    // Hello with one pane whose exited flag is 2
    body = vec![65u8];
    body.extend(VERSION.to_le_bytes());
    body.extend(1u32.to_le_bytes()); // one pane
    body.extend(1u32.to_le_bytes()); // pane id
    body.extend(0u32.to_le_bytes()); // program
    body.extend(0u32.to_le_bytes()); // cwd
    body.extend(1u16.to_le_bytes());
    body.extend(1u16.to_le_bytes());
    body.push(2); // bad flag
    body.extend(0i32.to_le_bytes());
    body.extend(0u32.to_le_bytes()); // layout
    let mut d = Decoder::new();
    d.push(&framed(&body));
    assert!(matches!(
        d.next::<ToWindow>(),
        Err(ProtoError::Malformed(_))
    ));
}

#[test]
fn zero_length_frame_is_malformed() {
    let mut d = Decoder::new();
    d.push(&0u32.to_le_bytes());
    assert!(matches!(
        d.next::<ToHolder>(),
        Err(ProtoError::Malformed(_))
    ));
}

#[test]
fn arbitrary_bytes_never_panic() {
    // Cheap deterministic fuzz: every decode must return, never panic.
    let mut x: u64 = 0x9e3779b97f4a7c15;
    for _ in 0..20_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let len = (x % 40) as usize;
        let mut body: Vec<u8> = (0..len)
            .map(|i| (x.rotate_left(i as u32 * 5) >> 3) as u8)
            .collect();
        if let Some(k) = body.first_mut() {
            *k = [1, 2, 3, 4, 5, 6, 7, 8, 65, 66, 67, 68, 69, 70, 71][(x % 15) as usize];
        }
        if let Some((kind, rest)) = body.split_first() {
            let _ = ToHolder::decode(*kind, rest);
            let _ = ToWindow::decode(*kind, rest);
        }
    }
}

#[test]
fn holder_hello_of_another_version_ignores_the_rest() {
    let mut body = vec![1u8];
    body.extend((VERSION + 1).to_le_bytes());
    body.extend([0xeeu8; 16]);
    let mut d = Decoder::new();
    d.push(&framed(&body));
    assert_eq!(
        d.next::<ToHolder>().unwrap(),
        Some(ToHolder::Hello {
            version: VERSION + 1
        })
    );
}

#[test]
fn window_hello_of_another_version_ignores_the_rest() {
    let mut body = vec![65u8];
    body.extend((VERSION + 1).to_le_bytes());
    body.extend([0xffu8; 7]); // not this version's layout
    let mut d = Decoder::new();
    d.push(&framed(&body));
    assert_eq!(
        d.next::<ToWindow>().unwrap(),
        Some(ToWindow::Hello {
            version: VERSION + 1,
            panes: vec![],
            layout: vec![]
        })
    );
}

#[test]
fn current_version_hello_with_trailing_byte_is_malformed() {
    let mut body = vec![1u8];
    body.extend(VERSION.to_le_bytes());
    body.push(0);
    let mut d = Decoder::new();
    d.push(&framed(&body));
    assert!(matches!(
        d.next::<ToHolder>(),
        Err(ProtoError::Malformed(_))
    ));

    let mut body = vec![65u8];
    body.extend(VERSION.to_le_bytes());
    body.extend(0u32.to_le_bytes()); // no panes
    body.extend(0u32.to_le_bytes()); // empty layout
    body.push(0);
    let mut d = Decoder::new();
    d.push(&framed(&body));
    assert!(matches!(
        d.next::<ToWindow>(),
        Err(ProtoError::Malformed(_))
    ));
}

#[test]
fn hello_shorter_than_a_version_is_malformed() {
    for kind in [1u8, 65] {
        let mut d = Decoder::new();
        d.push(&framed(&[kind, 1, 0, 0]));
        let r = if kind == 1 {
            d.next::<ToHolder>().map(|_| ())
        } else {
            d.next::<ToWindow>().map(|_| ())
        };
        assert!(matches!(r, Err(ProtoError::Malformed(_))));
    }
}
