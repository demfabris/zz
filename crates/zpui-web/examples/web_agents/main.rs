#![cfg_attr(target_family = "wasm", no_main)]

//! Three GPUI windows mounted among a page's own HTML, with their
//! accessibility trees mirrored into the page and `globalThis.zpui` driving
//! them. Build and serve with `crates/zpui-web/examples/web_agents/build.sh`.

#[cfg(not(target_family = "wasm"))]
fn main() {
    eprintln!("web_agents runs in a browser; see examples/web_agents/build.sh");
}

#[cfg(target_family = "wasm")]
#[allow(clippy::disallowed_methods)]
mod web {
    use std::borrow::Cow;
    use std::rc::Rc;

    use zpui::{
        App, Application, Context, FontWeight, Hsla, Role, SharedString, Toggled, Window,
        WindowOptions, div, prelude::*, px, rgb, size, text,
    };
    use zpui_web::WebPlatform;

    const ROW: f32 = 28.0;
    const CHROME: f32 = 132.0;

    struct Panel {
        title: SharedString,
        accent: Hsla,
        count: usize,
        enabled: bool,
        items: Vec<SharedString>,
        grows: bool,
    }

    impl Panel {
        fn content_height(&self) -> f32 {
            CHROME + self.items.len() as f32 * ROW
        }

        fn add_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
            let next = self.items.len() + 1;
            self.items.push(format!("Item {next}").into());
            if self.grows {
                let width = window.bounds().size.width;
                window.resize(size(width, px(self.content_height())));
            }
            cx.notify();
        }
    }

    impl Render for Panel {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let accent = self.accent;
            div()
                .id("panel")
                .role(Role::Group)
                .aria_label(self.title.clone())
                .size_full()
                .flex()
                .flex_col()
                .gap_3()
                .p_4()
                .bg(rgb(0x1e1e2e))
                .text_color(rgb(0xcdd6f4))
                .child(
                    div()
                        .id("title")
                        .role(Role::Heading)
                        .aria_level(2)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(text!(self.title.clone())),
                )
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .id("increment")
                                .role(Role::Button)
                                .aria_label(SharedString::from(format!("Count {}", self.count)))
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(accent)
                                .text_color(rgb(0x1e1e2e))
                                .cursor_pointer()
                                .hover(|style| style.opacity(0.85))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.count += 1;
                                    cx.notify();
                                }))
                                .child(text!(format!("Count {}", self.count))),
                        )
                        .child(
                            div()
                                .id("add")
                                .role(Role::Button)
                                .aria_label("Add item")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .bg(rgb(0x45475a))
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(0x585b70)))
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.add_item(window, cx)),
                                )
                                .child(text!("Add item")),
                        )
                        .child(
                            div()
                                .id("switch")
                                .role(Role::Switch)
                                .aria_label("Enabled")
                                .aria_toggled(if self.enabled {
                                    Toggled::True
                                } else {
                                    Toggled::False
                                })
                                .w(px(40.))
                                .h(px(22.))
                                .rounded_full()
                                .cursor_pointer()
                                .bg(if self.enabled {
                                    accent
                                } else {
                                    rgb(0x45475a).into()
                                })
                                .child(
                                    div()
                                        .size(px(18.))
                                        .mt(px(2.))
                                        .ml(px(if self.enabled { 20. } else { 2. }))
                                        .rounded_full()
                                        .bg(zpui::white()),
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.enabled = !this.enabled;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    div()
                        .id("items")
                        .role(Role::List)
                        .aria_label("Items")
                        .flex()
                        .flex_col()
                        .children(self.items.iter().enumerate().map(|(index, item)| {
                            div()
                                .id(("item", index))
                                .role(Role::ListItem)
                                .h(px(ROW))
                                .flex()
                                .items_center()
                                .child(text!(item.clone()))
                        })),
                )
        }
    }

    fn open(cx: &mut App, mount: &str, title: &str, accent: u32, items: usize, grows: bool) {
        let title: SharedString = title.to_owned().into();
        cx.open_window(
            WindowOptions {
                mount: Some(mount.to_owned().into()),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title(&title);
                let panel = Panel {
                    title,
                    accent: rgb(accent).into(),
                    count: 0,
                    enabled: false,
                    items: (1..=items).map(|n| format!("Item {n}").into()).collect(),
                    grows,
                };
                if grows {
                    let width = window.bounds().size.width;
                    window.resize(size(width, px(panel.content_height())));
                }
                cx.new(|_| panel)
            },
        )
        .expect("failed to open a mounted window");
    }

    #[wasm_bindgen::prelude::wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
        zpui_web::init_logging();
        let platform = Rc::new(WebPlatform::new(false));
        Application::with_platform(platform).run(|cx: &mut App| {
            cx.text_system()
                .add_fonts(vec![
                    Cow::Borrowed(include_bytes!(
                        "../../../zpui/assets/fonts/ibm-plex-sans/IBMPlexSans-Regular.ttf"
                    )),
                    Cow::Borrowed(include_bytes!(
                        "../../../zpui/assets/fonts/ibm-plex-sans/IBMPlexSans-SemiBold.ttf"
                    )),
                ])
                .expect("failed to load fonts");
            open(cx, "#first", "First window", 0x89b4fa, 2, false);
            open(cx, "#second", "Second window", 0xa6e3a1, 1, false);
            open(cx, "#third", "Growing window", 0xf9e2af, 1, true);
        });
    }
}
