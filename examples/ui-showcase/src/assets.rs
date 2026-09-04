use std::borrow::Cow;

pub(crate) fn inter_fonts() -> Vec<Cow<'static, [u8]>> {
    vec![
        Cow::Borrowed(include_bytes!("../assets/fonts/inter/InterVariable.ttf")),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/inter/InterVariable-Italic.ttf"
        )),
    ]
}
