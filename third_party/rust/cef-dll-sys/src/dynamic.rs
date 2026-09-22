use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use libloading::os::unix::{Library, RTLD_LOCAL, RTLD_NOW};
use upstream::*;

static FUNCTIONS: OnceLock<Functions> = OnceLock::new();
static LOAD_LOCK: Mutex<()> = Mutex::new(());

macro_rules! cef_functions {
    ($(fn $name:ident($($argument:ident: $argument_type:ty),* $(,)?) $(-> $result:ty)?;)*) => {
        struct Functions {
            $($name: unsafe extern "C" fn($($argument_type),*) $(-> $result)?,)*
            _library: Library,
        }

        impl Functions {
            unsafe fn open(path: &Path) -> Result<Self, String> {
                let library = unsafe { Library::open(Some(path), RTLD_NOW | RTLD_LOCAL) }
                    .map_err(|error| format!("{}: {error}", path.display()))?;
                Ok(Self {
                    $($name: unsafe {
                        *library.get::<unsafe extern "C" fn($($argument_type),*) $(-> $result)?>(
                            concat!(stringify!($name), "\0").as_bytes(),
                        ).map_err(|error| format!("{}: {error}", path.display()))?
                    },)*
                    _library: library,
                })
            }
        }

        $(
            const _: unsafe extern "C" fn($($argument_type),*) $(-> $result)? = upstream::$name;

            pub unsafe extern "C" fn $name($($argument: $argument_type),*) $(-> $result)? {
                let functions = FUNCTIONS.get().expect("CEF library must be loaded before calling its API");
                unsafe { (functions.$name)($($argument),*) }
            }
        )*
    };
}

include!("functions.rs");

pub fn is_library_loaded() -> bool {
    FUNCTIONS.get().is_some()
}

pub fn load_library() -> Result<(), String> {
    if is_library_loaded() {
        return Ok(());
    }
    let _guard = LOAD_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    if is_library_loaded() {
        return Ok(());
    }
    let path = library_path()?;
    let functions = unsafe { Functions::open(&path)? };
    let _ = FUNCTIONS.set(functions);
    Ok(())
}

fn library_path() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let directory = executable
        .parent()
        .ok_or("executable has no parent directory")?;
    let adjacent = directory.join("libcef.so");
    if adjacent.is_file() {
        return Ok(adjacent);
    }
    if directory.file_name().is_some_and(|name| name == "deps") {
        if let Some(parent) = directory.parent() {
            let development = parent.join("libcef.so");
            if development.is_file() {
                return Ok(development);
            }
        }
    }
    if let Some(directory) = crate::get_cef_dir() {
        let distribution = directory.join("libcef.so");
        if distribution.is_file() {
            return Ok(distribution);
        }
    }
    Ok(adjacent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_library_reports_its_path_without_publishing_functions() {
        let path = std::env::temp_dir()
            .join(format!("zz-cef-missing-{}", std::process::id()))
            .join("libcef.so");
        let error = unsafe { Functions::open(&path) }.err().unwrap();
        assert!(error.contains(path.to_str().unwrap()));
        assert!(!is_library_loaded());
    }
}
