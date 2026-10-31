//! Compile the app icon (data/mote.gresource.xml) into a `.gresource` blob
//! that is embedded in the binary and registered at startup (see `main.rs`).

fn main() {
    println!("cargo:rerun-if-changed=data/mote.gresource.xml");
    println!("cargo:rerun-if-changed=data/icons/hicolor");
    glib_build_tools::compile_resources(&["data"], "data/mote.gresource.xml", "mote.gresource");
}
