

#![cfg(feature = "kitty-graphics")]

use std::{
    cell::RefCell,
    mem::{ManuallyDrop, MaybeUninit},
};

use crate::{
    Terminal,
    alloc::{Allocator, Bytes, Object, Ref},
    error::{Error, Result, from_optional_result_uninit, from_result},
    ffi,
    selection::Selection,
};

#[doc(inline)]
pub use ffi::KittyGraphicsPlacementRenderInfo as PlacementRenderInfo;

#[derive(Debug)]
pub struct Graphics<'t> {
    inner: Ref<'t, ffi::KittyGraphicsImpl>,
}

#[derive(Debug)]
pub struct Image<'t> {
    inner: Ref<'t, ffi::KittyGraphicsImageImpl>,
}

#[derive(Debug)]
pub struct PlacementIterator<'alloc> {
    inner: Object<'alloc, ffi::KittyGraphicsPlacementIteratorImpl>,
}

#[derive(Debug)]
pub struct PlacementIteration<'t, 'alloc>(&'t mut PlacementIterator<'alloc>);

impl Terminal<'_, '_> {

    pub fn kitty_graphics(&self) -> Result<Graphics<'_>> {
        let inner = self.get::<ffi::KittyGraphics>(ffi::TerminalData::KITTY_GRAPHICS)?;
        Ok(Graphics {
            inner: Ref::new(inner)?,
        })
    }

    pub fn kitty_image_storage_limit(&self) -> Result<u64> {
        self.get(ffi::TerminalData::KITTY_IMAGE_STORAGE_LIMIT)
    }

    pub fn is_kitty_image_from_file_allowed(&self) -> Result<bool> {
        self.get(ffi::TerminalData::KITTY_IMAGE_MEDIUM_FILE)
    }

    pub fn is_kitty_image_from_temp_file_allowed(&self) -> Result<bool> {
        self.get(ffi::TerminalData::KITTY_IMAGE_MEDIUM_TEMP_FILE)
    }

    pub fn is_kitty_image_from_shared_mem_allowed(&self) -> Result<bool> {
        self.get(ffi::TerminalData::KITTY_IMAGE_MEDIUM_SHARED_MEM)
    }

    pub fn set_kitty_image_storage_limit(&mut self, limit: u64) -> Result<&mut Self> {
        self.set(ffi::TerminalOption::KITTY_IMAGE_STORAGE_LIMIT, &limit)?;
        Ok(self)
    }

    pub fn set_kitty_image_from_file_allowed(&mut self, allowed: bool) -> Result<&mut Self> {
        self.set(ffi::TerminalOption::KITTY_IMAGE_MEDIUM_FILE, &allowed)?;
        Ok(self)
    }

    pub fn set_kitty_image_from_temp_file_allowed(&mut self, allowed: bool) -> Result<&mut Self> {
        self.set(ffi::TerminalOption::KITTY_IMAGE_MEDIUM_TEMP_FILE, &allowed)?;
        Ok(self)
    }

    pub fn set_kitty_image_from_shared_mem_allowed(&mut self, allowed: bool) -> Result<&mut Self> {
        self.set(ffi::TerminalOption::KITTY_IMAGE_MEDIUM_SHARED_MEM, &allowed)?;
        Ok(self)
    }

    pub fn set_apc_max_bytes_kitty(&mut self, max: Option<usize>) -> Result<&mut Self> {
        self.set_optional(ffi::TerminalOption::APC_MAX_BYTES_KITTY, max.as_ref())?;
        Ok(self)
    }
}

impl<'t> Graphics<'t> {
    fn get<T>(&self, tag: ffi::KittyGraphicsData::Type) -> Result<T> {
        let mut value = MaybeUninit::<T>::zeroed();
        let result = unsafe {
            ffi::ghostty_kitty_graphics_get(self.inner.as_raw(), tag, value.as_mut_ptr().cast())
        };
        from_result(result)?;
        Ok(unsafe { value.assume_init() })
    }

    pub fn image(&self, id: u32) -> Option<Image<'t>> {
        let image = unsafe { ffi::ghostty_kitty_graphics_image(self.inner.as_raw(), id) };

        Some(Image {
            inner: Ref::new(image.cast_mut()).ok()?,
        })
    }

    pub fn generation(&self) -> Result<u64> {
        self.get(ffi::KittyGraphicsData::GENERATION)
    }
}

impl<'t> Image<'t> {
    fn get<T>(&self, tag: ffi::KittyGraphicsImageData::Type) -> Result<T> {
        let mut value = MaybeUninit::<T>::zeroed();
        let result = unsafe {
            ffi::ghostty_kitty_graphics_image_get(
                self.inner.as_raw(),
                tag,
                value.as_mut_ptr().cast(),
            )
        };

        from_result(result)?;

        Ok(unsafe { value.assume_init() })
    }

    pub fn id(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsImageData::ID)
    }

    pub fn number(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsImageData::NUMBER)
    }

    pub fn width(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsImageData::WIDTH)
    }

    pub fn height(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsImageData::HEIGHT)
    }

    pub fn generation(&self) -> Result<u64> {
        self.get(ffi::KittyGraphicsImageData::GENERATION)
    }

    pub fn format(&self) -> Result<ImageFormat> {
        self.get::<ffi::KittyImageFormat::Type>(ffi::KittyGraphicsImageData::FORMAT)
            .and_then(|v| v.try_into().map_err(|_| Error::InvalidValue))
    }

    pub fn compression(&self) -> Result<Compression> {
        self.get::<ffi::KittyImageCompression::Type>(ffi::KittyGraphicsImageData::COMPRESSION)
            .and_then(|v| v.try_into().map_err(|_| Error::InvalidValue))
    }

    pub fn data(&self) -> Result<&'t [u8]> {
        let ptr = self.get::<*const u8>(ffi::KittyGraphicsImageData::DATA_PTR)?;
        let len = self.get::<usize>(ffi::KittyGraphicsImageData::DATA_LEN)?;

        Ok(unsafe { std::slice::from_raw_parts(ptr, len) })
    }
}

impl<'alloc> PlacementIterator<'alloc> {

    pub fn new() -> Result<Self> {

        unsafe { Self::new_inner(std::ptr::null()) }
    }

    pub fn new_with_alloc<'ctx: 'alloc>(alloc: &'alloc Allocator<'ctx>) -> Result<Self> {

        unsafe { Self::new_inner(alloc.to_raw()) }
    }
    unsafe fn new_inner(alloc: *const ffi::Allocator) -> Result<Self> {
        let mut inner: ffi::KittyGraphicsPlacementIterator = std::ptr::null_mut();
        let result =
            unsafe { ffi::ghostty_kitty_graphics_placement_iterator_new(alloc, &raw mut inner) };
        from_result(result)?;
        Ok(Self {
            inner: Object::new(inner)?,
        })
    }

    pub fn update(&mut self, graphics: &Graphics<'_>) -> Result<PlacementIteration<'_, 'alloc>> {
        let result = unsafe {
            ffi::ghostty_kitty_graphics_get(
                graphics.inner.as_raw(),
                ffi::KittyGraphicsData::PLACEMENT_ITERATOR,
                (&raw mut self.inner).cast(),
            )
        };
        from_result(result)?;
        Ok(PlacementIteration(self))
    }
}

impl Drop for PlacementIterator<'_> {
    fn drop(&mut self) {
        unsafe {
            ffi::ghostty_kitty_graphics_placement_iterator_free(self.inner.as_raw());
        }
    }
}

impl<'t, 'alloc> PlacementIteration<'t, 'alloc> {

    pub fn next(&mut self) -> Option<&Self> {
        if unsafe { ffi::ghostty_kitty_graphics_placement_next(self.0.inner.as_raw()) } {
            Some(self)
        } else {
            None
        }
    }

    fn set<T>(
        &self,
        tag: ffi::KittyGraphicsPlacementIteratorOption::Type,
        value: &T,
    ) -> Result<()> {
        let result = unsafe {
            ffi::ghostty_kitty_graphics_placement_iterator_set(
                self.0.inner.as_raw(),
                tag,
                std::ptr::from_ref(value).cast(),
            )
        };
        from_result(result)
    }
    fn get<T>(&self, tag: ffi::KittyGraphicsPlacementData::Type) -> Result<T> {
        let mut value = MaybeUninit::<T>::zeroed();
        let result = unsafe {
            ffi::ghostty_kitty_graphics_placement_get(
                self.0.inner.as_raw(),
                tag,
                value.as_mut_ptr().cast(),
            )
        };

        from_result(result)?;

        Ok(unsafe { value.assume_init() })
    }

    pub fn set_layer(&self, layer: Layer) -> Result<()> {
        self.set::<ffi::KittyPlacementLayer::Type>(
            ffi::KittyGraphicsPlacementIteratorOption::LAYER,
            &layer.into(),
        )
    }

    pub fn pixel_size(
        &self,
        image: &Image<'t>,
        terminal: &'t Terminal<'_, '_>,
    ) -> Result<PixelSize> {
        let mut size = PixelSize::default();
        let result = unsafe {
            ffi::ghostty_kitty_graphics_placement_pixel_size(
                self.0.inner.as_raw(),
                image.inner.as_raw(),
                terminal.inner.as_raw(),
                &raw mut size.width,
                &raw mut size.height,
            )
        };
        from_result(result)?;
        Ok(size)
    }

    pub fn grid_size(&self, image: &Image<'t>, terminal: &'t Terminal<'_, '_>) -> Result<GridSize> {
        let mut size = GridSize::default();
        let result = unsafe {
            ffi::ghostty_kitty_graphics_placement_grid_size(
                self.0.inner.as_raw(),
                image.inner.as_raw(),
                terminal.inner.as_raw(),
                &raw mut size.cols,
                &raw mut size.rows,
            )
        };
        from_result(result)?;
        Ok(size)
    }

    pub fn viewport_pos(
        &self,
        image: &Image<'t>,
        terminal: &'t Terminal<'_, '_>,
    ) -> Result<Option<ViewportPos>> {
        let mut pos = ViewportPos::default();
        let result = unsafe {
            ffi::ghostty_kitty_graphics_placement_viewport_pos(
                self.0.inner.as_raw(),
                image.inner.as_raw(),
                terminal.inner.as_raw(),
                &raw mut pos.col,
                &raw mut pos.row,
            )
        };
        from_optional_result_uninit(result, MaybeUninit::new(pos))
    }

    pub fn source_rect(&self, image: &Image<'t>) -> Result<SourceRect> {
        let mut rect = SourceRect::default();
        let result = unsafe {
            ffi::ghostty_kitty_graphics_placement_source_rect(
                self.0.inner.as_raw(),
                image.inner.as_raw(),
                &raw mut rect.x,
                &raw mut rect.y,
                &raw mut rect.width,
                &raw mut rect.height,
            )
        };
        from_result(result)?;
        Ok(rect)
    }

    pub fn rect(&self, image: &Image<'t>, terminal: &'t Terminal<'_, '_>) -> Result<Selection<'t>> {
        let mut sel = MaybeUninit::<ffi::Selection>::zeroed();
        let result = unsafe {
            ffi::ghostty_kitty_graphics_placement_rect(
                self.0.inner.as_raw(),
                image.inner.as_raw(),
                terminal.inner.as_raw(),
                sel.as_mut_ptr(),
            )
        };
        from_result(result)?;

        Ok(unsafe { Selection::from_raw(sel.assume_init()) })
    }

    pub fn placement_render_info(
        &self,
        image: &Image<'t>,
        terminal: &'t Terminal<'_, '_>,
    ) -> Result<PlacementRenderInfo> {
        let mut info = ffi::sized!(PlacementRenderInfo);
        let result = unsafe {
            ffi::ghostty_kitty_graphics_placement_render_info(
                self.0.inner.as_raw(),
                image.inner.as_raw(),
                terminal.inner.as_raw(),
                &raw mut info,
            )
        };
        from_result(result)?;
        Ok(info)
    }

    pub fn image_id(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsPlacementData::IMAGE_ID)
    }

    pub fn placement_id(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsPlacementData::PLACEMENT_ID)
    }

    pub fn is_virtual(&self) -> Result<bool> {
        self.get(ffi::KittyGraphicsPlacementData::IS_VIRTUAL)
    }

    pub fn x_offset(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsPlacementData::X_OFFSET)
    }

    pub fn y_offset(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsPlacementData::Y_OFFSET)
    }

    pub fn source_x(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsPlacementData::SOURCE_X)
    }

    pub fn source_y(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsPlacementData::SOURCE_Y)
    }

    pub fn source_width(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsPlacementData::SOURCE_WIDTH)
    }

    pub fn source_height(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsPlacementData::SOURCE_HEIGHT)
    }

    pub fn columns(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsPlacementData::COLUMNS)
    }

    pub fn rows(&self) -> Result<u32> {
        self.get(ffi::KittyGraphicsPlacementData::ROWS)
    }

    pub fn z(&self) -> Result<i32> {
        self.get(ffi::KittyGraphicsPlacementData::Z)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PixelSize {

    pub width: u32,

    pub height: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GridSize {

    pub cols: u32,

    pub rows: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewportPos {

    pub col: i32,

    pub row: i32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceRect {

    pub x: u32,

    pub y: u32,

    pub width: u32,

    pub height: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, int_enum::IntEnum)]
#[repr(u32)]
pub enum Layer {

    #[default]
    All = ffi::KittyPlacementLayer::ALL,

    BelowBg = ffi::KittyPlacementLayer::BELOW_BG,

    BelowText = ffi::KittyPlacementLayer::BELOW_TEXT,

    AboveText = ffi::KittyPlacementLayer::ABOVE_TEXT,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, int_enum::IntEnum)]
#[non_exhaustive]
#[repr(u32)]
#[expect(missing_docs, reason = "missing upstream docs")]
pub enum ImageFormat {
    #[default]
    Rgb = ffi::KittyImageFormat::RGB,
    Rgba = ffi::KittyImageFormat::RGBA,
    Png = ffi::KittyImageFormat::PNG,
    GrayAlpha = ffi::KittyImageFormat::GRAY_ALPHA,
    Gray = ffi::KittyImageFormat::GRAY,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, int_enum::IntEnum)]
#[non_exhaustive]
#[repr(u32)]
#[expect(missing_docs, reason = "missing upstream docs")]
pub enum Compression {
    #[default]
    None = ffi::KittyImageCompression::NONE,
    ZlibDeflate = ffi::KittyImageCompression::ZLIB_DEFLATE,
}

thread_local! {
    static DECODE_PNG: RefCell<Option<Box<dyn DecodePng>>> = RefCell::new(None);
}

pub fn set_png_decoder(f: Option<Box<dyn DecodePng>>) -> Result<()> {
    unsafe extern "C" fn callback(
        _userdata: *mut std::ffi::c_void,
        allocator: *const ffi::Allocator,
        data: *const u8,
        data_len: usize,
        out: *mut ffi::SysImage,
    ) -> bool {
        DECODE_PNG.with_borrow_mut(|decoder| {
            let Some(decoder) = decoder else {
                return false;
            };

            let alloc = unsafe { Allocator::from_raw(allocator) };
            let data = unsafe { std::slice::from_raw_parts(data, data_len) };

            match decoder.decode_png(&alloc, data) {
                Some(result) => {

                    let mut result = ManuallyDrop::new(result);
                    unsafe {
                        *out = ffi::SysImage {
                            width: result.width,
                            height: result.height,
                            data: result.data.as_mut_ptr(),
                            data_len: result.data.len(),
                        }
                    };
                    true
                }
                None => false,
            }
        })
    }

    let ptr: ffi::SysDecodePngFn = match f {
        None => None,
        Some(_) => Some(callback),
    };
    DECODE_PNG.replace(f);

    crate::sys_set(
        ffi::SysOption::GHOSTTY_SYS_OPT_DECODE_PNG,
        ptr.map_or(std::ptr::null(), |p| p as *const std::ffi::c_void),
    )
}

pub trait DecodePng: 'static {

    fn decode_png<'alloc>(
        &mut self,
        alloc: &'alloc Allocator<'_>,
        data: &[u8],
    ) -> Option<DecodedImage<'alloc>>;
}

#[cfg(all(feature = "kitty-graphics", feature = "png"))]
#[derive(Clone, Debug)]
pub struct RustPngDecoder {
    buf: Vec<u8>,
}
#[cfg(all(feature = "kitty-graphics", feature = "png"))]
impl DecodePng for RustPngDecoder {
    fn decode_png<'alloc>(
        &mut self,
        alloc: &'alloc Allocator<'_>,
        data: &[u8],
    ) -> Option<DecodedImage<'alloc>> {
        use png::{Decoder, Transformations};
        use std::io::Cursor;

        let mut decoder = Decoder::new(Cursor::new(data));

        decoder.set_transformations(Transformations::ALPHA | Transformations::STRIP_16);

        let mut frame = decoder.read_info().ok()?;
        let buf_size = frame.output_buffer_size()?;
        if buf_size > self.buf.capacity() {
            self.buf.reserve(buf_size - self.buf.capacity());
        }
        self.buf.fill(0);

        let info = frame.next_frame(&mut self.buf).ok()?;

        let mut bytes = Bytes::new_with_alloc(alloc, info.buffer_size()).ok()?;
        bytes.copy_from_slice(&self.buf[..info.buffer_size()]);
        frame.finish().ok()?;

        Some(DecodedImage {
            width: info.width,
            height: info.height,
            data: bytes,
        })
    }
}

#[derive(Debug)]
pub struct DecodedImage<'alloc> {

    pub width: u32,

    pub height: u32,

    pub data: Bytes<'alloc>,
}
impl From<DecodedImage<'_>> for ffi::SysImage {
    fn from(mut value: DecodedImage<'_>) -> Self {
        Self {
            width: value.width,
            height: value.height,
            data: value.data.as_mut_ptr(),
            data_len: value.data.len(),
        }
    }
}
