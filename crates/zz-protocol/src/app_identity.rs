pub const DEVELOPMENT: bool = match option_env!("ZZ_DEV_BUILD") {
    Some(value) => matches!(value.as_bytes(), b"1"),
    None => false,
};

pub const DIRECTORY: &str = if DEVELOPMENT { "zz-dev" } else { "zz" };
pub const DISPLAY_NAME: &str = if DEVELOPMENT { "zz Dev" } else { "zz" };
pub const MACOS_BUNDLE_ID: &str = if DEVELOPMENT {
    "dev.zz.app.dev"
} else {
    "dev.zz.app"
};
