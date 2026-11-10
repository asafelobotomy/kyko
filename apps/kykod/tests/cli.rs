use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kykod"))
}

#[test]
fn version_flag_prints_name_and_version() {
    let out = bin().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        concat!("kykod ", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_flag_exits_2() {
    assert_eq!(bin().arg("--nope").output().unwrap().status.code(), Some(2));
}
