use std::{mem::forget, path::Path};

use anyhow::Result;

pub(super) fn load(path: impl AsRef<Path>, global: bool) -> Result<()> {
    let path = path.as_ref();
    // Bundled Android runtimes are named by bare soname; the linker resolves them
    // through the app's native library search path and there is no file to
    // canonicalize. Fall back to the raw path so `dlopen` receives the soname.
    let path = match dunce::canonicalize(path) {
        Ok(path) => path,
        Err(_) if cfg!(target_os = "android") => path.to_owned(),
        Err(error) => return Err(error.into()),
    };
    let library = unsafe { open(&path, global) }?;
    forget(library);
    Ok(())
}

#[cfg(windows)]
unsafe fn open(path: &Path, _global: bool) -> Result<libloading::Library, libloading::Error> {
    use libloading::os::windows::{
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR, LOAD_LIBRARY_SEARCH_SYSTEM32, Library,
    };
    unsafe {
        Library::load_with_flags(
            path.as_os_str(),
            LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
        .map(Into::into)
    }
}

#[cfg(not(windows))]
unsafe fn open(path: &Path, global: bool) -> Result<libloading::Library, libloading::Error> {
    use libloading::os::unix::{Library, RTLD_GLOBAL, RTLD_LAZY, RTLD_LOCAL};
    let visibility = if global { RTLD_GLOBAL } else { RTLD_LOCAL };
    unsafe { Library::open(Some(path), RTLD_LAZY | visibility).map(Into::into) }
}
