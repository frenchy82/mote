//! Compile the app icon (data/mote.gresource.xml) into a `.gresource` blob
//! that is embedded in the binary and registered at startup (see `main.rs`),
//! and copy the credits files for the About window to read.

fn main() {
    // data/CONTRIBUTORS and data/TRANSLATORS arrive with their first names
    // (tools/check-docs.py turns an empty one down); until then, nobody.
    // The whole of data/ is watched, so the files' arrival is noticed.
    println!("cargo:rerun-if-changed=data");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    for name in ["CONTRIBUTORS", "TRANSLATORS"] {
        let text = std::fs::read_to_string(format!("data/{name}")).unwrap_or_default();
        std::fs::write(out.join(name), text).expect("writes the credits file");
    }
    println!("cargo:rerun-if-changed=data/mote.gresource.xml");
    println!("cargo:rerun-if-changed=data/icons/hicolor");
    glib_build_tools::compile_resources(&["data"], "data/mote.gresource.xml", "mote.gresource");
}
