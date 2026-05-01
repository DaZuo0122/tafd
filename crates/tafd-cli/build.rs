use std::fs;
use std::path::Path;

fn main() {
    // Copy assets/ next to the binary so `cargo run` and the packaged app work.
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let _profile = std::env::var("PROFILE").expect("PROFILE not set");

    // OUT_DIR: target/debug/build/tafd-cli-*/out
    // Target dir: target/debug/ or target/release/
    let target_dir = Path::new(&out_dir)
        .ancestors()
        .nth(3)
        .expect("Failed to locate target dir");

    let src = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"));
    let dst = target_dir.join("assets");

    if src.exists() {
        if let Err(e) = copy_dir_all(src, &dst) {
            eprintln!("Warning: failed to copy assets to target dir: {e}");
        }
    }
}

fn copy_dir_all(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(entry.path(), dst.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}
