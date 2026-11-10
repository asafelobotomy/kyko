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
    let moc_sg = out.join("moc_sgitem.cpp");
    let header_sg = std::fs::canonicalize("cpp/sgitem.h").unwrap();
    assert!(std::process::Command::new(moc).arg(&header_sg).arg("-o").arg(&moc_sg).status().unwrap().success(), "moc failed");
    // Shaders for the glyph material, baked with Qt's shader tool.
    let qsb = ["/usr/lib/qt6/bin/qsb", "/usr/lib64/qt6/bin/qsb", "/usr/bin/qsb"]
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
        .expect("Qt 6 qsb (qt6-shadertools) not found");
    for name in ["glyph.vert", "glyph.frag"] {
        let ok = std::process::Command::new(qsb)
            .arg("--qt6")
            .arg("-b")
            .arg("-o")
            .arg(out.join(format!("{name}.qsb")))
            .arg(format!("shaders/{name}"))
            .status()
            .unwrap()
            .success();
        assert!(ok, "qsb failed for {name}");
        println!("cargo::rerun-if-changed=shaders/{name}");
    }
    let shader_dir = format!("\"{}\"", out.display());
    println!("cargo::rerun-if-changed=cpp/sgitem.h");
    println!("cargo::rerun-if-changed=cpp/sgitem.cpp");
    println!("cargo::rerun-if-changed=cpp/griditem.h");
    println!("cargo::rerun-if-changed=cpp/griditem.cpp");
    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("org.kyko.spike").qml_files([QmlFile::from("qml/Main.qml")]),
    )
    .qt_module("Gui")
    .qt_module("Quick")
    .files(["src/toggle.rs", "src/bridge.rs"])
    .cpp_file("cpp/griditem.cpp")
    .cpp_file(moc_out)
    .cpp_file("cpp/sgitem.cpp")
    .cpp_file(moc_sg);
    // SAFETY: only adds a warning flag; GCC 16 warns about Qt's own headers.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.flag_if_supported("-Wno-sfinae-incomplete");
            cc.define("KYKO_SHADER_DIR", Some(shader_dir.as_str()));
        })
    };
    builder.build();
}
