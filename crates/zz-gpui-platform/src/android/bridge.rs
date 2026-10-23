use crate::android::{platform, text_input, window};
use jni::{
    JNIEnv, JavaVM, NativeMethod,
    objects::{
        GlobalRef, JByteArray, JClass, JFloatArray, JIntArray, JObject, JObjectArray, JString,
        JValue,
    },
    sys::{JNI_FALSE, JNI_TRUE, JNI_VERSION_1_6, jboolean, jfloat, jint, jintArray, jstring},
};
use std::{
    cell::{Cell, RefCell},
    ffi::c_void,
    path::PathBuf,
    sync::OnceLock,
};

static VM: OnceLock<JavaVM> = OnceLock::new();
static MAIN: OnceLock<fn()> = OnceLock::new();
static FILES: OnceLock<PathBuf> = OnceLock::new();

thread_local! {
    static ACTIVITY: RefCell<Option<GlobalRef>> = const { RefCell::new(None) };
    static VIEW: RefCell<Option<GlobalRef>> = const { RefCell::new(None) };
    static CONFIGURATION: Cell<Configuration> = const { Cell::new(Configuration::DEFAULT) };
    static CORNER_RADIUS: Cell<f32> = const { Cell::new(0.0) };
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Configuration {
    pub density: f32,
    pub font_scale: f32,
    pub night: bool,
    pub smallest_width_dp: i32,
    pub hardware_keyboard: bool,
    pub refresh_rate: f32,
    pub reduce_motion: bool,
    pub high_contrast: bool,
}

impl Configuration {
    const DEFAULT: Self = Self {
        density: 1.0,
        font_scale: 1.0,
        night: true,
        smallest_width_dp: 400,
        hardware_keyboard: false,
        refresh_rate: 60.0,
        reduce_motion: false,
        high_contrast: false,
    };
}

pub(crate) fn configuration() -> Configuration {
    CONFIGURATION.get()
}

#[derive(Clone, Copy, Debug)]
pub struct Accessibility {
    pub reduce_motion: bool,
    pub increase_contrast: bool,
    pub text_scale: f32,
}

pub fn accessibility() -> Accessibility {
    let configuration = configuration();
    Accessibility {
        reduce_motion: configuration.reduce_motion,
        increase_contrast: configuration.high_contrast,
        text_scale: configuration.font_scale,
    }
}

pub fn phone() -> bool {
    configuration().smallest_width_dp < 600
}

/// The radius of the screen's rounded corners in points, when it has them.
pub fn display_corner_radius() -> Option<f32> {
    let radius = CORNER_RADIUS.get() / configuration().density;
    (radius > 0.0).then_some(radius)
}

pub(crate) fn set_corner_radius(pixels: f32) {
    CORNER_RADIUS.set(pixels);
}

/// The app's private files directory, once the activity has started.
pub fn files_dir() -> Option<PathBuf> {
    FILES.get().cloned()
}

/// Turns the status and navigation bar glyphs light for a dark backdrop, dark for
/// a light one, or back to following the app's appearance with `None`.
pub fn set_status_bar_on_dark(dark: Option<bool>) {
    let dark = dark.unwrap_or_else(|| platform::appearance_is_dark());
    call_activity(
        "setLightSystemBars",
        "(Z)V",
        &[JValue::Bool(u8::from(!dark))],
    );
}

/// Registers the natives and remembers `main`, which builds and runs the app the
/// first time the activity is created. Call it from the library's `JNI_OnLoad`.
pub fn on_load(vm: *mut c_void, main: fn()) -> jint {
    let Ok(vm) = (unsafe { JavaVM::from_raw(vm.cast()) }) else {
        return jni::sys::JNI_ERR;
    };
    let registered = vm
        .get_env()
        .and_then(|mut env| env.register_native_methods("dev/zz/android/ZzNative", &natives()));
    if let Err(error) = registered {
        log::error!("registering natives failed: {error}");
        return jni::sys::JNI_ERR;
    }
    VM.set(vm).ok();
    MAIN.set(main).ok();
    JNI_VERSION_1_6
}

fn native(name: &str, sig: &str, fn_ptr: *mut c_void) -> NativeMethod {
    NativeMethod {
        name: name.into(),
        sig: sig.into(),
        fn_ptr,
    }
}

fn natives() -> Vec<NativeMethod> {
    vec![
        native(
            "onCreate",
            "(Ldev/zz/android/ZzActivity;Ldev/zz/android/ZzView;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;)V",
            on_create as *mut c_void,
        ),
        native("onDestroy", "()V", on_destroy as *mut c_void),
        native("onResume", "()V", on_resume as *mut c_void),
        native("onPause", "()V", on_pause as *mut c_void),
        native(
            "onConfiguration",
            "(FFZIZFZZ)V",
            on_configuration as *mut c_void,
        ),
        native("onUrl", "(Ljava/lang/String;)V", on_url as *mut c_void),
        native("onBack", "()V", on_back as *mut c_void),
        native(
            "surfaceCreated",
            "(Landroid/view/Surface;)V",
            surface_created as *mut c_void,
        ),
        native(
            "surfaceChanged",
            "(Landroid/view/Surface;II)V",
            surface_changed as *mut c_void,
        ),
        native(
            "surfaceRedrawNeeded",
            "()V",
            surface_redraw_needed as *mut c_void,
        ),
        native("surfaceDestroyed", "()V", surface_destroyed as *mut c_void),
        native("insets", "(IIIIIIF)V", insets as *mut c_void),
        native("touch", "(II[I[F[F[III)V", touch as *mut c_void),
        native("pointer", "(IFFFFII)Z", pointer as *mut c_void),
        native("key", "(IIIIIII)Z", key as *mut c_void),
        native("focusChanged", "(Z)V", focus_changed as *mut c_void),
        native("editorInfo", "()[I", editor_info as *mut c_void),
        native(
            "imeCommit",
            "(Ljava/lang/String;I)V",
            ime_commit as *mut c_void,
        ),
        native(
            "imeCompose",
            "(Ljava/lang/String;I)V",
            ime_compose as *mut c_void,
        ),
        native(
            "imeComposingRegion",
            "(II)V",
            ime_composing_region as *mut c_void,
        ),
        native("imeFinish", "()V", ime_finish as *mut c_void),
        native("imeDelete", "(II)V", ime_delete as *mut c_void),
        native("imeText", "(II)Ljava/lang/String;", ime_text as *mut c_void),
        native("imeSelection", "()[I", ime_selection as *mut c_void),
        native("imeSetSelection", "(II)V", ime_set_selection as *mut c_void),
        native("imeAction", "(I)V", ime_action as *mut c_void),
        native("imeMenuAction", "(I)Z", ime_menu_action as *mut c_void),
        native("keyRow", "(IZ)V", key_row as *mut c_void),
    ]
}

fn string(env: &mut JNIEnv, value: &JString) -> Option<String> {
    if value.is_null() {
        return None;
    }
    env.get_string(value).ok().map(Into::into)
}

fn floats(env: &mut JNIEnv, array: &JFloatArray) -> Vec<f32> {
    let length = env.get_array_length(array).unwrap_or(0).max(0) as usize;
    let mut values = vec![0.0; length];
    env.get_float_array_region(array, 0, &mut values).ok();
    values
}

fn ints(env: &mut JNIEnv, array: &JIntArray) -> Vec<i32> {
    let length = env.get_array_length(array).unwrap_or(0).max(0) as usize;
    let mut values = vec![0; length];
    env.get_int_array_region(array, 0, &mut values).ok();
    values
}

fn int_array(env: &mut JNIEnv, values: &[i32]) -> jintArray {
    let Ok(array) = env.new_int_array(values.len() as i32) else {
        return std::ptr::null_mut();
    };
    env.set_int_array_region(&array, 0, values).ok();
    array.into_raw()
}

fn boolean(value: bool) -> jboolean {
    if value { JNI_TRUE } else { JNI_FALSE }
}

extern "system" fn on_create<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    activity: JObject<'local>,
    view: JObject<'local>,
    files: JString<'local>,
    cache: JString<'local>,
    url: JString<'local>,
    environment: JObjectArray<'local>,
) {
    let (Ok(activity), Ok(view)) = (env.new_global_ref(activity), env.new_global_ref(view)) else {
        return;
    };
    ACTIVITY.set(Some(activity));
    VIEW.set(Some(view));
    if let Some(files) = string(&mut env, &files) {
        if std::env::var_os("HOME").is_none_or(|home| home.is_empty() || home == "/") {
            unsafe { std::env::set_var("HOME", &files) };
        }
        FILES.set(PathBuf::from(files)).ok();
    }
    if let Some(cache) = string(&mut env, &cache)
        && std::env::var_os("TMPDIR").is_none()
    {
        unsafe { std::env::set_var("TMPDIR", cache) };
    }
    let count = env.get_array_length(&environment).unwrap_or(0);
    for index in (0..count - 1).step_by(2) {
        let pair = [index, index + 1].map(|index| {
            env.get_object_array_element(&environment, index)
                .ok()
                .map(JString::from)
                .and_then(|value| string(&mut env, &value))
        });
        if let [Some(key), Some(value)] = pair {
            unsafe { std::env::set_var(key, value) };
        }
    }
    let url = string(&mut env, &url);
    let main = MAIN.get().copied();
    platform::activity_created(main, url);
}

extern "system" fn on_destroy<'local>(_: JNIEnv<'local>, _: JClass<'local>) {
    platform::activity_destroyed();
    ACTIVITY.set(None);
    VIEW.set(None);
}

extern "system" fn on_resume<'local>(_: JNIEnv<'local>, _: JClass<'local>) {
    window::set_active(true);
}

extern "system" fn on_pause<'local>(_: JNIEnv<'local>, _: JClass<'local>) {
    window::set_active(false);
}

extern "system" fn on_configuration<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    density: jfloat,
    font_scale: jfloat,
    night: jboolean,
    smallest_width_dp: jint,
    hardware_keyboard: jboolean,
    refresh_rate: jfloat,
    reduce_motion: jboolean,
    high_contrast: jboolean,
) {
    let configuration = Configuration {
        density: if density > 0.0 { density } else { 1.0 },
        font_scale,
        night: night == JNI_TRUE,
        smallest_width_dp,
        hardware_keyboard: hardware_keyboard == JNI_TRUE,
        refresh_rate,
        reduce_motion: reduce_motion == JNI_TRUE,
        high_contrast: high_contrast == JNI_TRUE,
    };
    log::info!("configuration: {configuration:?}");
    let previous = CONFIGURATION.replace(configuration);
    if previous != configuration {
        window::configuration_changed(previous, configuration);
    }
}

extern "system" fn on_url<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    url: JString<'local>,
) {
    if let Some(url) = string(&mut env, &url) {
        platform::open_urls(vec![url]);
    }
}

extern "system" fn on_back<'local>(_: JNIEnv<'local>, _: JClass<'local>) {
    window::back();
}

extern "system" fn surface_created<'local>(
    env: JNIEnv<'local>,
    _: JClass<'local>,
    surface: JObject<'local>,
) {
    let native = unsafe {
        ndk_sys::ANativeWindow_fromSurface(env.get_raw().cast(), surface.as_raw().cast())
    };
    if let Some(native) = std::ptr::NonNull::new(native) {
        platform::surface_created(native);
    }
}

extern "system" fn surface_changed<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    _surface: JObject<'local>,
    width: jint,
    height: jint,
) {
    window::surface_changed(width, height);
}

extern "system" fn surface_redraw_needed<'local>(_: JNIEnv<'local>, _: JClass<'local>) {
    window::redraw_now();
}

extern "system" fn surface_destroyed<'local>(_: JNIEnv<'local>, _: JClass<'local>) {
    platform::surface_destroyed();
}

extern "system" fn insets<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    top: jint,
    left: jint,
    bottom: jint,
    right: jint,
    ime: jint,
    ime_rest: jint,
    corner_radius: jfloat,
) {
    set_corner_radius(corner_radius);
    window::set_insets([top, left, bottom, right], ime, ime_rest);
}

extern "system" fn touch<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    action: jint,
    index: jint,
    ids: JIntArray<'local>,
    xs: JFloatArray<'local>,
    ys: JFloatArray<'local>,
    tools: JIntArray<'local>,
    buttons: jint,
    meta: jint,
) {
    let ids = ints(&mut env, &ids);
    let xs = floats(&mut env, &xs);
    let ys = floats(&mut env, &ys);
    let tools = ints(&mut env, &tools);
    let pointers = ids
        .iter()
        .zip(xs.iter().zip(ys.iter()))
        .zip(tools.iter())
        .map(|((id, (x, y)), tool)| window::Pointer {
            id: *id,
            x: *x,
            y: *y,
            tool: *tool,
        })
        .collect::<Vec<_>>();
    window::touch(action, index as usize, &pointers, buttons, meta);
}

extern "system" fn pointer<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    action: jint,
    x: jfloat,
    y: jfloat,
    scroll_x: jfloat,
    scroll_y: jfloat,
    buttons: jint,
    meta: jint,
) -> jboolean {
    boolean(window::pointer(
        action, x, y, scroll_x, scroll_y, buttons, meta,
    ))
}

extern "system" fn key<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    action: jint,
    code: jint,
    _scan_code: jint,
    meta: jint,
    character: jint,
    base: jint,
    repeat: jint,
) -> jboolean {
    boolean(window::key(
        action,
        code,
        meta,
        character as u32,
        base as u32,
        repeat,
    ))
}

extern "system" fn focus_changed<'local>(_: JNIEnv<'local>, _: JClass<'local>, focused: jboolean) {
    window::set_focused(focused == JNI_TRUE);
}

extern "system" fn editor_info<'local>(mut env: JNIEnv<'local>, _: JClass<'local>) -> jintArray {
    let info = text_input::editor_info();
    int_array(&mut env, &info)
}

extern "system" fn ime_commit<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    text: JString<'local>,
    cursor: jint,
) {
    if let Some(text) = string(&mut env, &text) {
        text_input::commit(&text, cursor);
    }
}

extern "system" fn ime_compose<'local>(
    mut env: JNIEnv<'local>,
    _: JClass<'local>,
    text: JString<'local>,
    cursor: jint,
) {
    let text = string(&mut env, &text).unwrap_or_default();
    text_input::compose(&text, cursor);
}

extern "system" fn ime_composing_region<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    start: jint,
    end: jint,
) {
    text_input::composing_region(start, end);
}

extern "system" fn ime_finish<'local>(_: JNIEnv<'local>, _: JClass<'local>) {
    text_input::finish();
}

extern "system" fn ime_delete<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    before: jint,
    after: jint,
) {
    text_input::delete(before, after);
}

extern "system" fn ime_text<'local>(
    env: JNIEnv<'local>,
    _: JClass<'local>,
    start: jint,
    end: jint,
) -> jstring {
    match text_input::text(start, end) {
        Some(text) => env
            .new_string(text)
            .map_or(std::ptr::null_mut(), JString::into_raw),
        None => std::ptr::null_mut(),
    }
}

extern "system" fn ime_selection<'local>(mut env: JNIEnv<'local>, _: JClass<'local>) -> jintArray {
    match text_input::selection() {
        Some(selection) => int_array(&mut env, &selection),
        None => std::ptr::null_mut(),
    }
}

extern "system" fn ime_set_selection<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    start: jint,
    end: jint,
) {
    text_input::set_selection(start, end);
}

extern "system" fn ime_action<'local>(_: JNIEnv<'local>, _: JClass<'local>, action: jint) {
    text_input::editor_action(action);
}

extern "system" fn ime_menu_action<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    id: jint,
) -> jboolean {
    boolean(text_input::menu_action(id))
}

extern "system" fn key_row<'local>(
    _: JNIEnv<'local>,
    _: JClass<'local>,
    index: jint,
    selected: jboolean,
) {
    window::key_row_key(index, selected == JNI_TRUE);
}

/// Runs `f` with the main thread's JNI environment in its own local frame.
pub(crate) fn with_env<R>(f: impl FnOnce(&mut JNIEnv) -> jni::errors::Result<R>) -> Option<R> {
    let vm = VM.get()?;
    let mut env = vm.get_env().ok()?;
    let result = env.with_local_frame(16, |env| f(env));
    if env.exception_check().unwrap_or(false) {
        env.exception_describe().ok();
        env.exception_clear().ok();
    }
    result.ok()
}

/// Runs `f` on the main thread's JNI environment with the activity, inside a
/// local reference frame. `None` before the activity exists or when a JNI call
/// fails; a pending Java exception is logged and cleared.
pub fn with_activity<R>(
    f: impl FnOnce(&mut JNIEnv, &JObject) -> jni::errors::Result<R>,
) -> Option<R> {
    let activity = ACTIVITY.with_borrow(Clone::clone)?;
    with_env(|env| f(env, activity.as_obj()))
}

/// Loads one of the app's classes through the activity's class loader, which
/// works from native callbacks that have no Java frame of the app on the stack.
pub fn load_class<'local>(
    env: &mut JNIEnv<'local>,
    activity: &JObject,
    name: &str,
) -> jni::errors::Result<JClass<'local>> {
    let loader = env
        .call_method(activity, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
        .l()?;
    let name = env.new_string(name.replace('/', "."))?;
    let class = env
        .call_method(
            &loader,
            "loadClass",
            "(Ljava/lang/String;)Ljava/lang/Class;",
            &[JValue::Object(&name)],
        )?
        .l()?;
    Ok(JClass::from(class))
}

/// Runs `f` with the activity's `ZzView`, the view GPUI draws in.
pub(crate) fn with_view<R>(f: impl FnOnce(&mut JNIEnv, &JObject) -> R) -> Option<R> {
    let view = VIEW.with_borrow(Clone::clone)?;
    let vm = VM.get()?;
    let mut env = vm.get_env().ok()?;
    Some(f(&mut env, view.as_obj()))
}

pub(crate) fn call_activity(name: &str, sig: &str, args: &[JValue]) {
    with_activity(|env, activity| env.call_method(activity, name, sig, args).map(drop));
}

pub(crate) fn set_keyboard_visible(visible: bool) {
    call_activity(
        "setKeyboardVisible",
        "(Z)V",
        &[JValue::Bool(u8::from(visible))],
    );
}

pub(crate) fn restart_input() {
    call_activity("restartInput", "()V", &[]);
}

pub(crate) fn set_key_row(wanted: bool) {
    call_activity("setKeyRow", "(Z)V", &[JValue::Bool(u8::from(wanted))]);
}

pub(crate) fn clear_key_row_latch() {
    call_activity("clearKeyRowLatch", "()V", &[]);
}

pub(crate) fn update_selection(selection: [i32; 4]) {
    call_activity("updateSelection", "(IIII)V", &selection.map(JValue::Int));
}

pub(crate) fn set_keep_screen_on(on: bool) {
    call_activity("setKeepScreenOn", "(Z)V", &[JValue::Bool(u8::from(on))]);
}

pub(crate) fn set_back_enabled(enabled: bool) {
    call_activity("setBackEnabled", "(Z)V", &[JValue::Bool(u8::from(enabled))]);
}

pub(crate) fn open_url(url: &str) {
    with_activity(|env, activity| {
        let url = env.new_string(url)?;
        env.call_method(
            activity,
            "openUrl",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&url)],
        )
        .map(drop)
    });
}

pub(crate) fn show_text_menu(x: f32, y: f32) {
    call_activity(
        "showTextMenu",
        "(FF)V",
        &[JValue::Float(x), JValue::Float(y)],
    );
}

pub(crate) fn set_clipboard(text: Option<&str>, image: Option<(&[u8], &str)>) {
    with_activity(|env, activity| {
        let text = match text {
            Some(text) => JObject::from(env.new_string(text)?),
            None => JObject::null(),
        };
        let (bytes, kind) = match image {
            Some((bytes, kind)) => (
                JObject::from(env.byte_array_from_slice(bytes)?),
                JObject::from(env.new_string(kind)?),
            ),
            None => (JObject::null(), JObject::null()),
        };
        env.call_method(
            activity,
            "setClipboard",
            "(Ljava/lang/String;[BLjava/lang/String;)V",
            &[
                JValue::Object(&text),
                JValue::Object(&bytes),
                JValue::Object(&kind),
            ],
        )
        .map(drop)
    });
}

pub(crate) fn clipboard_text() -> Option<String> {
    with_activity(|env, activity| {
        let value = env
            .call_method(activity, "clipboardText", "()Ljava/lang/String;", &[])?
            .l()?;
        if value.is_null() {
            return Ok(None);
        }
        let value = JString::from(value);
        Ok(env.get_string(&value).ok().map(Into::into))
    })
    .flatten()
}

pub(crate) fn clipboard_image() -> Option<(String, Vec<u8>)> {
    with_activity(|env, activity| {
        let value = env
            .call_method(activity, "clipboardImage", "()[Ljava/lang/Object;", &[])?
            .l()?;
        if value.is_null() {
            return Ok(None);
        }
        let pair = JObjectArray::from(value);
        let kind = JString::from(env.get_object_array_element(&pair, 0)?);
        let bytes = JByteArray::from(env.get_object_array_element(&pair, 1)?);
        let kind: String = env.get_string(&kind)?.into();
        Ok(Some((kind, env.convert_byte_array(&bytes)?)))
    })
    .flatten()
}
