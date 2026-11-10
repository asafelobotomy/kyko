use cxx_qt_build::{CxxQtBuilder, QmlFile, QmlModule};

fn main() {
    let builder = CxxQtBuilder::new_qml_module(
        QmlModule::new("org.kyko.spike").qml_files([QmlFile::from("qml/Main.qml")]),
    )
    .qt_module("Gui")
    .qt_module("Quick")
    .files(["src/toggle.rs"]);
    // SAFETY: only adds a warning flag; GCC 16 warns about Qt's own headers.
    let builder = unsafe {
        builder.cc_builder(|cc| {
            cc.flag_if_supported("-Wno-sfinae-incomplete");
        })
    };
    builder.build();
}
