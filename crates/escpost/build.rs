use std::path::Path;

fn main() {
    let index = Path::new("frontend/dist/index.html");
    println!("cargo:rerun-if-changed={}", index.display());
    if !index.is_file() {
        panic!(
            "frontend bundle missing at {}; run a supported Docker or Just build command",
            index.display()
        );
    }
}
