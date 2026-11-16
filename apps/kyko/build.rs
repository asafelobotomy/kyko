//! Builds the C++ grid item: runs moc over its header (cxx-qt-build only runs moc for its own
//! QObjects) and bakes the glyph shaders with Qt's shader tool.
use std::path::{Path, PathBuf};
use std::process::Command;

use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

/// `qmake -query NAME`, with the qmake cxx-qt-build itself uses ($QMAKE, else qmake6).
fn qmake_query(name: &str) -> Option<PathBuf> {
    let qmake = std::env::var("QMAKE").unwrap_or_else(|_| "qmake6".into());
    let out = Command::new(qmake).args(["-query", name]).output().ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

fn tool(name: &str, dirs: &[Option<PathBuf>]) -> PathBuf {
    dirs.iter()
        .flatten()
        .map(|d| d.join(name))
        .find(|p| p.exists())
        .unwrap_or_else(|| panic!("Qt 6 tool {name} not found (qt6-base, qt6-shadertools)"))
}

fn main() {
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let libexecs = qmake_query("QT_HOST_LIBEXECS");
    let bins = qmake_query("QT_HOST_BINS");
    let moc = tool(
        "moc",
        &[libexecs.clone(), bins.clone(), Some("/usr/lib/qt6".into())],
    );
    let qsb = tool("qsb", &[bins, libexecs, Some("/usr/lib/qt6/bin".into())]);

    let header = std::fs::canonicalize("cpp/gridview.h").unwrap();
    let moc_out = out.join("moc_gridview.cpp");
    let ok = Command::new(moc)
        .arg(&header)
        .arg("-o")
        .arg(&moc_out)
        .status()
        .unwrap()
        .success();
    assert!(ok, "moc failed");

    for name in ["glyph.vert", "glyph.frag"] {
        let ok = Command::new(&qsb)
            .args(["--qt6", "-b", "-o"])
            .arg(out.join(format!("{name}.qsb")))
            .arg(Path::new("shaders").join(name))
            .status()
            .unwrap()
            .success();
        assert!(ok, "qsb failed for {name}");
        println!("cargo::rerun-if-changed=shaders/{name}");
    }
    for f in ["cpp/gridview.h", "cpp/gridview.cpp"] {
        println!("cargo::rerun-if-changed={f}");
    }
    // The shaders go into the binary as a Qt resource: a path into the build tree would not
    // survive packaging.
    let qrc = out.join("shaders.qrc");
    std::fs::write(
        &qrc,
        format!(
            "<RCC><qresource prefix=\"/kyko/shaders\"><file alias=\"glyph.vert.qsb\">{v}</file><file alias=\"glyph.frag.qsb\">{f}</file></qresource></RCC>",
            v = out.join("glyph.vert.qsb").display(),
            f = out.join("glyph.frag.qsb").display()
        ),
    )
    .unwrap();
    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("org.kyko.app").qml_files([QmlFile::from("qml/Main.qml")]),
    )
    .qt_module("Gui")
    .qt_module("Quick")
    .files(["src/bridge.rs"])
    .cpp_file("cpp/gridview.cpp")
    .cpp_file(moc_out)
    .qrc(&qrc);
    // SAFETY: only adds a warning flag for the C++ file.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.flag_if_supported("-Wno-sfinae-incomplete");
        })
    };
    builder.build();
}
