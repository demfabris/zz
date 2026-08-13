//! Proof-of-concept: gpui rendering on iOS.
//! Build: cargo build -p zz-gpui-ios --example poc --target aarch64-apple-ios-sim

#[cfg(target_os = "ios")]
mod poc {
    use gpui::{
        App, Application, Context, IntoElement, Render, Window, WindowOptions, div, prelude::*, px,
        rgb,
    };
    use std::rc::Rc;
    use zz_gpui_ios::IosPlatform;

    struct Poc;

    impl Render for Poc {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(0x101014))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_4()
                        .p_8()
                        .rounded_2xl()
                        .bg(rgb(0x1c1c22))
                        .border_1()
                        .border_color(rgb(0x33333d))
                        .child(div().size(px(64.)).rounded_2xl().bg(rgb(0x7c5cff)))
                        .child(
                            div()
                                .text_2xl()
                                .text_color(rgb(0xf0f0f5))
                                .child("zz on iOS"),
                        )
                        .child(
                            div()
                                .text_color(rgb(0x8a8a96))
                                .child("gpui · Metal · UIKit"),
                        ),
                )
        }
    }

    pub fn main() {
        Application::with_platform(Rc::new(IosPlatform::new())).run(|cx: &mut App| {
            cx.open_window(WindowOptions::default(), |_, cx| cx.new(|_| Poc))
                .unwrap();
        });
    }
}

#[cfg(target_os = "ios")]
fn main() {
    poc::main();
}

#[cfg(not(target_os = "ios"))]
fn main() {}
