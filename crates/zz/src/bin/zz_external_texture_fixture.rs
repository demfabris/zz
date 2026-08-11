use std::process::ExitCode;

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
use std::env;

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
mod fixture {
    use std::{
        env,
        process::ExitCode,
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };

    use env_logger::{Builder, Env, WriteStyle};
    use gpui::{
        App, Bounds, Context, DevicePixels, ObjectFit, Pixels, Render, Size, WgpuDeviceContext,
        Window, WindowBounds, WindowOptions, div, external_texture, prelude::*, px, rgb, size,
        wgpu,
    };
    use gpui_platform::application;

    const DEFAULT_SECONDS: u64 = 5;
    const TEXTURE_LOGICAL_WIDTH: f32 = 560.0;
    const TEXTURE_LOGICAL_HEIGHT: f32 = 350.0;
    const CLIP_WIDTH: f32 = 420.0;
    const CLIP_HEIGHT: f32 = 260.0;

    pub fn main() -> ExitCode {
        let seconds = match parse_args(env::args().skip(1)) {
            Ok(Some(seconds)) => seconds,
            Ok(None) => {
                println!("{}", usage());
                return ExitCode::SUCCESS;
            }
            Err(error) => {
                eprintln!("{error}\n{}", usage());
                return ExitCode::from(2);
            }
        };

        let _ = Builder::from_env(Env::default().default_filter_or("wgpu=warn,gpui=info"))
            .write_style(WriteStyle::Never)
            .try_init();
        run(Duration::from_secs(seconds));
        ExitCode::SUCCESS
    }

    fn usage() -> &'static str {
        "usage: zz_external_texture_fixture [--seconds N]"
    }

    fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Option<u64>, String> {
        let mut seconds = DEFAULT_SECONDS;
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--help" | "-h" => return Ok(None),
                "--seconds" => {
                    let value = args
                        .next()
                        .ok_or_else(|| "--seconds requires a value".to_owned())?;
                    seconds = value
                        .parse::<u64>()
                        .ok()
                        .filter(|seconds| *seconds > 0)
                        .ok_or_else(|| format!("invalid positive duration: {value}"))?;
                }
                _ => return Err(format!("unknown argument: {argument}")),
            }
        }
        Ok(Some(seconds))
    }

    fn run(duration: Duration) {
        application().run(move |cx: &mut App| {
            let frame_count = Arc::new(AtomicU64::new(0));
            let bounds = Bounds::centered(None, size(px(720.0), px(500.0)), cx);
            let frame_count_for_view = frame_count.clone();
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |window, cx| {
                    let gpu = window
                        .wgpu_device_context()
                        .expect("the external-texture fixture requires GPUI's wgpu renderer");
                    cx.new(|_| {
                        ExternalTextureFixture::new(
                            gpu,
                            window.scale_factor(),
                            frame_count_for_view,
                        )
                    })
                },
            )
            .expect("could not open the external-texture fixture window");

            cx.on_window_closed(|cx, _| cx.quit()).detach();
            cx.spawn(async move |cx| {
                cx.background_executor().timer(duration).await;
                println!(
                    "zz external-texture fixture: clean timed exit after {:.3}s ({} uploaded frames)",
                    duration.as_secs_f64(),
                    frame_count.load(Ordering::Relaxed),
                );
                cx.update(|cx| cx.quit());
            })
            .detach();
            cx.activate(true);
        });
    }

    struct ExternalTextureFixture {
        gpu: WgpuDeviceContext,
        texture: wgpu::Texture,
        pixels: Vec<u8>,
        logical_size: Size<Pixels>,
        device_size: Size<DevicePixels>,
        scale_factor: f32,
        frame: u64,
        frame_count: Arc<AtomicU64>,
    }

    impl ExternalTextureFixture {
        fn new(gpu: WgpuDeviceContext, scale_factor: f32, frame_count: Arc<AtomicU64>) -> Self {
            let logical_size = size(px(TEXTURE_LOGICAL_WIDTH), px(TEXTURE_LOGICAL_HEIGHT));
            let device_size = logical_size.to_device_pixels(scale_factor);
            let texture = create_texture(&gpu.device, device_size);
            let pixels = allocate_pixels(device_size);
            log_dimensions(logical_size, device_size, scale_factor);
            Self {
                gpu,
                texture,
                pixels,
                logical_size,
                device_size,
                scale_factor,
                frame: 0,
                frame_count,
            }
        }

        fn update_scale_factor(&mut self, scale_factor: f32) {
            if (self.scale_factor - scale_factor).abs() < f32::EPSILON {
                return;
            }
            self.scale_factor = scale_factor;
            self.device_size = self.logical_size.to_device_pixels(scale_factor);
            self.texture = create_texture(&self.gpu.device, self.device_size);
            self.pixels = allocate_pixels(self.device_size);
            log_dimensions(self.logical_size, self.device_size, scale_factor);
        }

        fn upload_frame(&mut self) {
            let width = u32::try_from(self.device_size.width.0).expect("positive texture width");
            let height = u32::try_from(self.device_size.height.0).expect("positive texture height");
            paint_bgra_pattern(&mut self.pixels, width, height, self.frame);
            self.gpu.queue.write_texture(
                self.texture.as_image_copy(),
                &self.pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            self.frame += 1;
            self.frame_count.store(self.frame, Ordering::Relaxed);
        }
    }

    impl Render for ExternalTextureFixture {
        #[allow(
            clippy::disallowed_methods,
            reason = "the fixture uses fixed proof colors to validate external texture compositing"
        )]
        fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            self.update_scale_factor(window.scale_factor());
            self.upload_frame();
            window.request_animation_frame();

            div()
                .flex()
                .flex_col()
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .bg(rgb(0x10_17_22))
                .text_color(rgb(0xe8_ee_f7))
                .child("GPUI app-provided BGRA8 wgpu texture")
                .child(format!(
                    "logical {}x{} · device {}x{} · scale {:.3}",
                    f32::from(self.logical_size.width),
                    f32::from(self.logical_size.height),
                    self.device_size.width.0,
                    self.device_size.height.0,
                    self.scale_factor,
                ))
                .child(
                    div()
                        .relative()
                        .w(px(CLIP_WIDTH))
                        .h(px(CLIP_HEIGHT))
                        .overflow_hidden()
                        .border_2()
                        .border_color(rgb(0xf2_c1_4e))
                        .bg(rgb(0x22_2d_3d))
                        .child(
                            external_texture(self.texture.clone())
                                .object_fit(ObjectFit::Fill)
                                .absolute()
                                .left(px(-70.0))
                                .top(px(-45.0))
                                .w(self.logical_size.width)
                                .h(self.logical_size.height),
                        )
                        .child(
                            div()
                                .absolute()
                                .right(px(0.0))
                                .top(px(0.0))
                                .w(px(92.0))
                                .h_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(rgb(0x11_18_27).opacity(0.82))
                                .text_color(rgb(0xff_ff_ff))
                                .child("occluder"),
                        ),
                )
                .child("Gold border is the overflow-hidden clip; the dark panel tests occlusion.")
        }
    }

    fn create_texture(device: &wgpu::Device, size: Size<DevicePixels>) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("zz_external_texture_fixture"),
            size: wgpu::Extent3d {
                width: u32::try_from(size.width.0).expect("positive texture width"),
                height: u32::try_from(size.height.0).expect("positive texture height"),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    }

    fn allocate_pixels(size: Size<DevicePixels>) -> Vec<u8> {
        let width = usize::try_from(size.width.0).expect("positive texture width");
        let height = usize::try_from(size.height.0).expect("positive texture height");
        vec![0; width * height * 4]
    }

    fn log_dimensions(
        logical_size: Size<Pixels>,
        device_size: Size<DevicePixels>,
        scale_factor: f32,
    ) {
        println!(
            "zz external-texture fixture: logical_size={}x{} device_pixel_size={}x{} scale_factor={scale_factor:.3}",
            f32::from(logical_size.width),
            f32::from(logical_size.height),
            device_size.width.0,
            device_size.height.0,
        );
        println!(
            "zz external-texture fixture: clip={CLIP_WIDTH}x{CLIP_HEIGHT} logical px, texture offset=(-70,-45)"
        );
    }

    fn paint_bgra_pattern(pixels: &mut [u8], width: u32, height: u32, frame: u64) {
        let phase = u32::try_from(frame % u64::from(width)).expect("phase fits u32");
        let color_phase = u32::try_from(frame % 256).expect("color phase fits u32");
        let checker_phase = u32::try_from((frame / 8) % 2).expect("checker phase fits u32");
        for y in 0..height {
            for x in 0..width {
                let offset = usize::try_from((y * width + x) * 4).expect("pixel offset fits usize");
                let checker = ((x / 32) + (y / 32) + checker_phase) % 2;
                let mut red = u8::try_from(((x * 255 / width) + color_phase * 3) % 256)
                    .expect("red channel fits u8");
                let mut green = u8::try_from(((y * 255 / height) + color_phase * 2) % 256)
                    .expect("green channel fits u8");
                let mut blue = if checker == 0 { 48 } else { 176 };
                let direct_distance = x.abs_diff(phase);
                let wrapped_distance = width - direct_distance;
                if direct_distance.min(wrapped_distance) < 10 {
                    red = 255;
                    green = 255;
                    blue = 255;
                }
                pixels[offset] = blue;
                pixels[offset + 1] = green;
                pixels[offset + 2] = red;
                pixels[offset + 3] = 255;
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_duration() {
            assert_eq!(parse_args(std::iter::empty()), Ok(Some(DEFAULT_SECONDS)));
            assert_eq!(
                parse_args(["--seconds", "2"].map(str::to_owned).into_iter()),
                Ok(Some(2))
            );
            assert!(parse_args(["--seconds", "0"].map(str::to_owned).into_iter()).is_err());
        }

        #[test]
        fn pattern_is_opaque_bgra_and_animated() {
            let mut first = vec![0; 32 * 32 * 4];
            let mut second = first.clone();
            paint_bgra_pattern(&mut first, 32, 32, 0);
            paint_bgra_pattern(&mut second, 32, 32, 16);
            assert!(first.chunks_exact(4).all(|pixel| pixel[3] == 255));
            assert_ne!(first, second);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn main() -> ExitCode {
    fixture::main()
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
fn main() -> ExitCode {
    let _ = env::args_os();
    eprintln!("zz_external_texture_fixture requires GPUI's Linux wgpu backend");
    ExitCode::FAILURE
}
