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

mod framing;
mod malformed;
