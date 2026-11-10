use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    // moc for the hand-written C++ item (cxx-qt-build only runs moc for its own QObjects).
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let moc_out = out.join("moc_griditem.cpp");
    let header = std::fs::canonicalize("cpp/griditem.h").unwrap();
    let moc = ["/usr/lib/qt6/moc", "/usr/lib/qt6/libexec/moc", "/usr/lib64/qt6/libexec/moc"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
        .expect("Qt 6 moc not found");
    let ok = std::process::Command::new(moc).arg(&header).arg("-o").arg(&moc_out).status().unwrap().success();
    assert!(ok, "moc failed");
    println!("cargo::rerun-if-changed=cpp/griditem.h");
    println!("cargo::rerun-if-changed=cpp/griditem.cpp");
    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("org.kyko.spike").qml_files([QmlFile::from("qml/Main.qml")]),
    )
    .qt_module("Gui")
    .qt_module("Quick")
    .files(["src/toggle.rs", "src/bridge.rs"])
    .cpp_file("cpp/griditem.cpp")
    .cpp_file(moc_out);
    // SAFETY: only adds a warning flag; GCC 16 warns about Qt's own headers.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.flag_if_supported("-Wno-sfinae-incomplete");
        })
    };
    builder.build();
}
