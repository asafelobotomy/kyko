use kyko_proto::{
    Decoder, Frame, MAX_FRAME, PaneInfo, ProtoError, SpawnSpec, ToHolder, ToWindow, VERSION,
};

fn rt<T: Frame + PartialEq + std::fmt::Debug + Clone>(f: T) {
    let mut bytes = Vec::new();
    f.encode(&mut bytes);
    let mut d = Decoder::new();
    for chunk in bytes.chunks(3) {
        d.push(chunk);
    }
    assert_eq!(d.next::<T>().unwrap(), Some(f));
    assert_eq!(d.next::<T>().unwrap(), None);
}

fn spec(shell: &str, env: Vec<(String, String)>) -> SpawnSpec {
    SpawnSpec {
        pane: 9,
        shell: shell.into(),
        args: vec!["-l".into(), "ünï".into()],
        cwd: "/srv/ü".into(),
        env,
        cols: 80,
        rows: 24,
        ring_bytes: 1 << 20,
    }
}

fn framed(body: &[u8]) -> Vec<u8> {
    let mut v = (body.len() as u32).to_le_bytes().to_vec();
    v.extend_from_slice(body);
    v
}

#[test]
fn every_frame_round_trips() {
    rt(ToHolder::Hello { version: VERSION });
    rt(ToHolder::Spawn(spec(
        "/bin/zsh",
        vec![
            ("TERM".into(), "xterm-256color".into()),
            ("Ж".into(), "".into()),
        ],
    )));
    rt(ToHolder::Spawn(spec("", vec![])));
    rt(ToHolder::Spawn(SpawnSpec {
        pane: 0,
        shell: String::new(),
        args: vec![],
        cwd: String::new(),
        env: vec![],
        cols: 0,
        rows: 0,
        ring_bytes: 0,
    }));
    rt(ToHolder::Input {
        pane: 1,
        bytes: vec![],
    });
    rt(ToHolder::Input {
        pane: 1,
        bytes: b"ls\r".to_vec(),
    });
    rt(ToHolder::Resize {
        pane: 2,
        cols: 65535,
        rows: 1,
    });
    rt(ToHolder::Close { pane: 3 });
    rt(ToHolder::Attach { pane: 4 });
    rt(ToHolder::SaveLayout { blob: vec![] });
    rt(ToHolder::SaveLayout {
        blob: vec![0, 255, 7],
    });
    rt(ToHolder::EndSessions { force: true });
    rt(ToHolder::EndSessions { force: false });

    rt(ToWindow::Hello {
        version: VERSION,
        panes: vec![],
        layout: vec![],
    });
    let info = |exited| PaneInfo {
        pane: 5,
        program: "日本語".into(),
        cwd: "/tmp".into(),
        cols: 100,
        rows: 30,
        exited,
    };
    rt(ToWindow::Hello {
        version: VERSION,
        panes: vec![
            info(None),
            info(Some(-9)),
            info(Some(0)),
            info(Some(i32::MAX)),
        ],
        layout: vec![1, 2, 3],
    });
    rt(ToWindow::Output {
        pane: 6,
        bytes: vec![],
    });
    rt(ToWindow::Output {
        pane: 6,
        bytes: (0..65536u32).map(|i| i as u8).collect(),
    });
    rt(ToWindow::Info {
        pane: 7,
        program: String::new(),
        cwd: String::new(),
    });
    rt(ToWindow::Info {
        pane: 7,
        program: "vim".into(),
        cwd: "/é".into(),
    });
    rt(ToWindow::Exited {
        pane: 8,
        status: -9,
    });
    rt(ToWindow::Exited { pane: 8, status: 0 });
    rt(ToWindow::Busy { panes: vec![] });
    rt(ToWindow::Busy {
        panes: vec![1, 2, u32::MAX],
    });
    rt(ToWindow::SpawnFailed {
        pane: 10,
        message: "no such file: ø".into(),
    });
    rt(ToWindow::Replayed {
        pane: 11,
        truncated: true,
    });
    rt(ToWindow::Replayed {
        pane: 11,
        truncated: false,
    });
}

#[test]
fn type_bytes_match_the_spec() {
    let kind = |f: &ToHolder| {
        let mut b = Vec::new();
        f.encode(&mut b);
        b[4]
    };
    assert_eq!(kind(&ToHolder::Hello { version: 1 }), 1);
    assert_eq!(kind(&ToHolder::Spawn(spec("s", vec![]))), 2);
    assert_eq!(
        kind(&ToHolder::Input {
            pane: 1,
            bytes: vec![]
        }),
        3
    );
    assert_eq!(
        kind(&ToHolder::Resize {
            pane: 1,
            cols: 1,
            rows: 1
        }),
        4
    );
    assert_eq!(kind(&ToHolder::Close { pane: 1 }), 5);
    assert_eq!(kind(&ToHolder::Attach { pane: 1 }), 6);
    assert_eq!(kind(&ToHolder::SaveLayout { blob: vec![] }), 7);
    assert_eq!(kind(&ToHolder::EndSessions { force: true }), 8);
    let wk = |f: &ToWindow| {
        let mut b = Vec::new();
        f.encode(&mut b);
        b[4]
    };
    assert_eq!(
        wk(&ToWindow::Hello {
            version: 1,
            panes: vec![],
            layout: vec![]
        }),
        65
    );
    assert_eq!(
        wk(&ToWindow::Output {
            pane: 1,
            bytes: vec![]
        }),
        66
    );
    assert_eq!(
        wk(&ToWindow::Info {
            pane: 1,
            program: String::new(),
            cwd: String::new()
        }),
        67
    );
    assert_eq!(wk(&ToWindow::Exited { pane: 1, status: 0 }), 68);
    assert_eq!(wk(&ToWindow::Busy { panes: vec![] }), 69);
    assert_eq!(
        wk(&ToWindow::SpawnFailed {
            pane: 1,
            message: String::new()
        }),
        70
    );
    assert_eq!(
        wk(&ToWindow::Replayed {
            pane: 1,
            truncated: false
        }),
        71
    );
}

#[test]
fn length_prefix_counts_type_and_body() {
    let mut b = Vec::new();
    ToHolder::Close { pane: 7 }.encode(&mut b);
    assert_eq!(b, [5, 0, 0, 0, 5, 7, 0, 0, 0]);
}

#[test]
fn two_frames_in_one_push_decode_in_order() {
    let a = ToHolder::Input {
        pane: 1,
        bytes: b"abc".to_vec(),
    };
    let b = ToHolder::Resize {
        pane: 1,
        cols: 120,
        rows: 40,
    };
    let mut bytes = Vec::new();
    a.encode(&mut bytes);
    b.encode(&mut bytes);
    let mut d = Decoder::new();
    d.push(&bytes);
    assert_eq!(d.next::<ToHolder>().unwrap(), Some(a));
    assert_eq!(d.next::<ToHolder>().unwrap(), Some(b));
    assert_eq!(d.next::<ToHolder>().unwrap(), None);
}

#[test]
fn incomplete_frame_is_none_not_an_error() {
    let mut b = Vec::new();
    ToHolder::Close { pane: 7 }.encode(&mut b);
    let mut d = Decoder::new();
    d.push(&b[..b.len() - 1]);
    assert_eq!(d.next::<ToHolder>().unwrap(), None);
    d.push(&b[b.len() - 1..]);
    assert_eq!(
        d.next::<ToHolder>().unwrap(),
        Some(ToHolder::Close { pane: 7 })
    );
}

#[test]
fn partial_length_prefix_is_none() {
    let mut d = Decoder::new();
    d.push(&[5, 0]);
    assert_eq!(d.next::<ToHolder>().unwrap(), None);
}

#[test]
fn oversized_length_is_rejected_before_buffering() {
    let mut d = Decoder::new();
    d.push(&(MAX_FRAME as u32 + 1).to_le_bytes());
    d.push(&[3]);
    assert!(matches!(d.next::<ToHolder>(), Err(ProtoError::TooLong(n)) if n == MAX_FRAME + 1));
    let mut d = Decoder::new();
    d.push(&u32::MAX.to_le_bytes());
    assert!(matches!(d.next::<ToHolder>(), Err(ProtoError::TooLong(_))));
}

#[test]
fn a_frame_of_exactly_max_frame_is_accepted() {
    // type byte + 4 (pane) + 4 (len) + payload
    let payload = vec![0xabu8; MAX_FRAME - 9];
    let f = ToHolder::Input {
        pane: 1,
        bytes: payload,
    };
    let mut b = Vec::new();
    f.encode(&mut b);
    assert_eq!(b.len(), 4 + MAX_FRAME);
    let mut d = Decoder::new();
    d.push(&b);
    assert_eq!(d.next::<ToHolder>().unwrap(), Some(f));
}

#[test]
fn unknown_type_is_rejected() {
    let mut d = Decoder::new();
    d.push(&1u32.to_le_bytes());
    d.push(&[200]);
    assert!(matches!(
        d.next::<ToHolder>(),
        Err(ProtoError::UnknownType(200))
    ));
    let mut d = Decoder::new();
    d.push(&framed(&[0]));
    assert!(matches!(
        d.next::<ToHolder>(),
        Err(ProtoError::UnknownType(0))
    ));
}

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
    body.extend(1u32.to_le_bytes());
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
    body.extend(1u32.to_le_bytes());
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
fn a_window_frame_type_is_unknown_to_the_holder() {
    let mut b = Vec::new();
    ToWindow::Busy { panes: vec![1] }.encode(&mut b);
    let mut d = Decoder::new();
    d.push(&b);
    assert!(matches!(
        d.next::<ToHolder>(),
        Err(ProtoError::UnknownType(69))
    ));
}

#[test]
fn a_holder_frame_type_is_unknown_to_the_window() {
    let mut b = Vec::new();
    ToHolder::Close { pane: 1 }.encode(&mut b);
    let mut d = Decoder::new();
    d.push(&b);
    assert!(matches!(
        d.next::<ToWindow>(),
        Err(ProtoError::UnknownType(5))
    ));
}

#[test]
fn a_bad_frame_does_not_block_the_next_one() {
    let mut bytes = framed(&[200]);
    ToHolder::Close { pane: 2 }.encode(&mut bytes);
    let mut d = Decoder::new();
    d.push(&bytes);
    assert!(d.next::<ToHolder>().is_err());
    assert_eq!(
        d.next::<ToHolder>().unwrap(),
        Some(ToHolder::Close { pane: 2 })
    );
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
    body.extend(2u32.to_le_bytes());
    body.extend([0xeeu8; 16]);
    let mut d = Decoder::new();
    d.push(&framed(&body));
    assert_eq!(
        d.next::<ToHolder>().unwrap(),
        Some(ToHolder::Hello { version: 2 })
    );
}

#[test]
fn window_hello_of_another_version_ignores_the_rest() {
    let mut body = vec![65u8];
    body.extend(2u32.to_le_bytes());
    body.extend([0xffu8; 7]); // not a version-1 layout
    let mut d = Decoder::new();
    d.push(&framed(&body));
    assert_eq!(
        d.next::<ToWindow>().unwrap(),
        Some(ToWindow::Hello {
            version: 2,
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
