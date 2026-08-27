use std::path::Path;

fn main() {
    let dist = Path::new("frontend/dist");
    println!("cargo:rerun-if-changed={}", dist.display());

    // Only debug builds of the server read the web app from disk at run time
    // (used by tests), while release builds embed the web app directly in the
    // binary.
    if std::env::var_os("CARGO_CFG_DEBUG_ASSERTIONS").is_some() {
        return;
    }

    if !dist.join("index.html").is_file() {
        panic!(
            "frontend bundle missing at {}; run a supported Docker or Just build command",
            dist.display()
        );
    }
}
