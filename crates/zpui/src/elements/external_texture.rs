use crate::{
    App, Bounds, DevicePixels, Element, ElementId, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, ObjectFit, Pixels, Size, Style, StyleRefinement, Styled, Window, size,
};
use refineable::Refineable;

/// The handle the renderer samples for an [`ExternalTexture`]: a view on the
/// wgpu backends, and the texture itself on DirectX, where the shader resource
/// view is created per draw.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
type ExternalTextureHandle = wgpu::TextureView;
#[cfg(target_os = "windows")]
type ExternalTextureHandle = windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

/// An element that paints a texture created on GPUI's device.
pub struct ExternalTexture {
    handle: ExternalTextureHandle,
    size: Size<DevicePixels>,
    object_fit: ObjectFit,
    style: StyleRefinement,
}

/// Creates an element that paints a texture created on GPUI's wgpu device.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub fn external_texture(texture: wgpu::Texture) -> ExternalTexture {
    let extent = texture.size();
    ExternalTexture {
        handle: texture.create_view(&wgpu::TextureViewDescriptor::default()),
        size: size(extent.width.into(), extent.height.into()),
        object_fit: ObjectFit::Contain,
        style: Default::default(),
    }
}

/// Creates an element that paints a texture created on GPUI's DirectX device.
///
/// The texture is sampled as premultiplied BGRA and must stay alive for as long
/// as the element does; holding the interface here keeps it referenced.
#[cfg(target_os = "windows")]
pub fn external_texture(
    texture: windows::Win32::Graphics::Direct3D11::ID3D11Texture2D,
) -> ExternalTexture {
    let desc = unsafe {
        let mut desc = windows::Win32::Graphics::Direct3D11::D3D11_TEXTURE2D_DESC::default();
        texture.GetDesc(&mut desc);
        desc
    };
    ExternalTexture {
        handle: texture,
        size: size((desc.Width as i32).into(), (desc.Height as i32).into()),
        object_fit: ObjectFit::Contain,
        style: Default::default(),
    }
}

impl ExternalTexture {
    /// Sets how the texture fits within the element's bounds.
    pub fn object_fit(mut self, object_fit: ObjectFit) -> Self {
        self.object_fit = object_fit;
        self
    }
}

impl Element for ExternalTexture {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.refine(&self.style);
        let layout_id = window.request_layout(style, [], cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
    }

    fn paint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        _: &mut App,
    ) {
        let mut style = Style::default();
        style.refine(&self.style);
        // The pane's rounded corners arrive as ordinary `.rounded_*` styling;
        // resolve them here so the surface pipeline can cut the texture with
        // the same curve quads use, instead of overflowing the pane's frame.
        let corner_radii = style.corner_radii.to_pixels(window.rem_size());
        let bounds = self.object_fit.get_bounds(bounds, self.size);
        window.paint_external_texture(bounds, corner_radii, self.handle.clone());
    }
}

impl IntoElement for ExternalTexture {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Styled for ExternalTexture {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}
