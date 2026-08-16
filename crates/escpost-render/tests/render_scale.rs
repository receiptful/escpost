use escpost_render::RenderScale;

#[test]
fn render_scale_accepts_only_product_supported_densities() {
    assert_eq!(RenderScale::new(1).unwrap().get(), 1);
    assert_eq!(RenderScale::new(2).unwrap().get(), 2);
    assert_eq!(RenderScale::new(3).unwrap().get(), 3);
    assert_eq!(
        RenderScale::new(0).unwrap_err().to_string(),
        "render scale must be between 1 and 3, got 0"
    );
    assert_eq!(
        RenderScale::new(4).unwrap_err().to_string(),
        "render scale must be between 1 and 3, got 4"
    );
}
