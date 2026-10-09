#[test]
fn test_derive_render() {
    use zz_gpui_macros::Render;

    #[derive(Render)]
    struct _Element;
}
