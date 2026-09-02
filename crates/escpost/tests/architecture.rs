use std::path::Path;

#[test]
fn http_printing_adapter_respects_feature_boundaries() {
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");

    assert!(
        source.join("features/printing/http.rs").is_file(),
        "the HTTP print adapter belongs beside the printing operation"
    );
    assert!(
        source.join("web/origin.rs").is_file(),
        "shared HTTP origin policy belongs to the web host"
    );
    assert!(
        !source.join("features/api").exists(),
        "API is an adapter, not an application feature"
    );
}
