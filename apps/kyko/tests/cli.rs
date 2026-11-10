use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_kyko"))
}

#[test]
fn version_flag_prints_name_and_version() {
    let out = bin().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        concat!("kyko ", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_flag_exits_2() {
    assert_eq!(bin().arg("--nope").output().unwrap().status.code(), Some(2));
}

#[test]
fn no_arguments_exits_1_with_a_message() {
    let out = bin().output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("nothing to do yet")
    );
}
