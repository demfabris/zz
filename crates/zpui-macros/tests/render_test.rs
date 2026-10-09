#[test]
fn test_derive_render() {
    use zpui_macros::Render;

    #[derive(Render)]
    struct _Element;
}
