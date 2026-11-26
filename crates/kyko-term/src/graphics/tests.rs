use super::*;

const AT: At = At {
    col: 2,
    row: 10,
    cell_w: 10,
    cell_h: 20,
    alt: false,
};

fn b64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.len();
        let v = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= n {
                out.push(T[((v >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn transmit_and_display_raw_rgba_moves_the_cursor_and_answers() {
    let mut s = Store::default();
    let px = vec![255u8; 25 * 30 * 4];
    let body = format!("a=T,f=32,s=25,v=30,i=7;{}", b64(&px));
    let out = s.command(body.as_bytes(), AT);
    assert_eq!(out.reply.as_deref(), Some(&b"\x1b_Gi=7;OK\x1b\\"[..]));
    assert_eq!(out.moved, Some((3, 2)), "25x30 px in 10x20 cells");
    let p = &s.placements[0];
    assert_eq!((p.col, p.row, p.cols, p.rows), (2, 10, 3, 2));
    assert_eq!(s.image(7).unwrap().width, 25);
}

#[test]
fn one_cell_size_keeps_the_shape() {
    let mut s = Store::default();
    let px = b64(&[0u8; 20 * 10 * 4]);
    s.command(
        format!("a=T,f=32,s=20,v=10,i=1,c=6,q=2;{px}").as_bytes(),
        AT,
    );
    let p = &s.placements[0];
    assert_eq!((p.cols, p.rows, p.size), (6, 2, (60, 30)));
}

#[test]
fn chunks_gather_then_decode() {
    let mut s = Store::default();
    let px = b64(&[1u8, 2, 3].repeat(4));
    let (a, b) = px.split_at(8);
    assert_eq!(
        s.command(format!("a=t,f=24,s=2,v=2,i=1,q=2,m=1;{a}").as_bytes(), AT),
        Outcome::default()
    );
    assert_eq!(
        s.command(format!("m=0;{b}").as_bytes(), AT),
        Outcome::default()
    );
    assert_eq!(&s.image(1).unwrap().rgba[..4], &[1, 2, 3, 255]);
    assert!(s.placements.is_empty(), "t only stores");
}

#[test]
fn put_query_delete_and_errors() {
    let mut s = Store::default();
    let px = b64(&[9u8; 4]);
    s.command(format!("a=t,f=32,s=1,v=1,i=5;{px}").as_bytes(), AT);
    let out = s.command(b"a=p,i=5,p=2,c=4,r=1,C=1", AT);
    assert_eq!(out.moved, None);
    assert_eq!(out.reply.as_deref(), Some(&b"\x1b_Gi=5,p=2;OK\x1b\\"[..]));
    // Moving placement 2 replaces it.
    s.command(b"a=p,i=5,p=2,q=1", At { col: 0, ..AT });
    assert_eq!(s.placements.len(), 1);
    assert_eq!(s.placements[0].col, 0);
    let q = s.command(format!("a=q,i=31,s=1,v=1,f=32;{px}").as_bytes(), AT);
    assert_eq!(q.reply.as_deref(), Some(&b"\x1b_Gi=31;OK\x1b\\"[..]));
    assert!(s.image(31).is_none(), "a query keeps nothing");
    let miss = s.command(b"a=p,i=99", AT);
    assert!(
        String::from_utf8(miss.reply.unwrap())
            .unwrap()
            .contains("ENOENT")
    );
    let shm = s.command(format!("a=t,t=s,i=3;{}", b64(b"/kyko")).as_bytes(), AT);
    assert!(
        String::from_utf8(shm.reply.unwrap())
            .unwrap()
            .contains("EINVAL")
    );
    s.command(b"a=d,d=I,i=5", AT);
    assert!(s.placements.is_empty());
    assert!(s.image(5).is_none());
}

#[test]
fn files_must_be_regular_and_temporary_ones_are_marked() {
    let mut s = Store::default();
    let dev = s.command(format!("a=t,t=f,i=1;{}", b64(b"/dev/zero")).as_bytes(), AT);
    assert!(
        String::from_utf8(dev.reply.unwrap())
            .unwrap()
            .contains("EINVAL")
    );
    let dir = tempfile::tempdir().unwrap();
    let plain = dir.path().join("x.rgba");
    std::fs::write(&plain, [7u8; 4]).unwrap();
    let path = plain.to_str().unwrap();
    let t = s.command(
        format!("a=t,t=t,f=32,s=1,v=1,i=2;{}", b64(path.as_bytes())).as_bytes(),
        AT,
    );
    assert!(
        String::from_utf8(t.reply.unwrap())
            .unwrap()
            .contains("EPERM")
    );
    let f = s.command(
        format!("a=t,t=f,f=32,s=1,v=1,i=2;{}", b64(path.as_bytes())).as_bytes(),
        AT,
    );
    assert_eq!(f.reply.as_deref(), Some(&b"\x1b_Gi=2;OK\x1b\\"[..]));
    assert!(plain.exists(), "a named file is left alone");
}

#[test]
fn a_temporary_name_cannot_reach_outside_the_temporary_folders() {
    let mut s = Store::default();
    // A file outside /tmp, named through /tmp with `..`, and through a link in /tmp.
    let home = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let victim = home.path().join("keep-tty-graphics-protocol.rgba");
    std::fs::write(&victim, [7u8; 4]).unwrap();
    let sub = std::path::Path::new("/tmp").join(format!(
        "kyko-test-{}-tty-graphics-protocol-dir",
        std::process::id()
    ));
    std::fs::create_dir_all(&sub).unwrap();
    let dotdot = format!("{}/../..{}", sub.display(), victim.display());
    let link = std::path::Path::new("/tmp").join(format!(
        "kyko-test-{}-tty-graphics-protocol",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(&victim, &link).unwrap();
    for path in [dotdot.as_str(), link.to_str().unwrap()] {
        let r = s.command(
            format!("a=t,t=t,f=32,s=1,v=1,i=3;{}", b64(path.as_bytes())).as_bytes(),
            AT,
        );
        let r = String::from_utf8(r.reply.unwrap()).unwrap();
        assert!(r.contains("EPERM") || r.contains("EBADF"), "{path}: {r}");
        assert!(victim.exists(), "{path} removed a file outside /tmp");
    }
    let _ = std::fs::remove_file(&link);
    let _ = std::fs::remove_dir(&sub);
    // /proc through /tmp/..
    let r = s.command(
        format!(
            "a=t,t=f,f=32,s=1,v=1,i=4;{}",
            b64(b"/tmp/../proc/self/environ")
        )
        .as_bytes(),
        AT,
    );
    assert!(
        String::from_utf8(r.reply.unwrap())
            .unwrap()
            .contains("EINVAL")
    );
}

#[test]
fn a_tiny_png_cannot_ask_for_a_huge_buffer() {
    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, 10_000, 10_000);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        // The header and a scrap of image data: decode must refuse before it makes the
        // 400 MB buffer the header asks for.
        let mut w = enc.write_header().unwrap();
        w.write_chunk(png::chunk::IDAT, &[0x78, 0x9c, 0x03, 0x00])
            .unwrap();
        std::mem::forget(w);
    }
    let c = Command::parse("f=100");
    let e = decode(&c, png_bytes).unwrap_err();
    assert!(e.contains("too large"), "{e}");
}

#[test]
fn png_decodes() {
    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, 2, 1);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[255, 0, 0, 0, 255, 0]).unwrap();
    }
    let c = Command::parse("f=100");
    let (w, h, rgba) = decode(&c, png_bytes).unwrap();
    assert_eq!((w, h), (2, 1));
    assert_eq!(rgba, [255, 0, 0, 255, 0, 255, 0, 255]);
}
