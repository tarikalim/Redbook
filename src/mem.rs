// Process-level memory settings that must be in place before the allocator starts.

/// macOS: relaunches once with `MallocSpaceEfficient=1`, which hands freed malloc pages
/// back to the kernel (~9 MB less footprint at idle). The bundle sets it via LSEnvironment.
#[cfg(target_os = "macos")]
pub fn ensure_space_efficient_malloc() {
    use std::os::unix::process::CommandExt as _;
    const VAR: &str = "MallocSpaceEfficient";
    if std::env::var_os(VAR).is_some() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut args = std::env::args_os();
    let argv0 = args.next().unwrap_or_else(|| exe.clone().into());
    // exec only returns on failure; then run on with the default allocator settings.
    let _ = std::process::Command::new(&exe)
        .arg0(argv0)
        .args(args)
        .env(VAR, "1")
        .exec();
}

#[cfg(not(target_os = "macos"))]
pub fn ensure_space_efficient_malloc() {}

/// macOS: caps CoreGraphics' glyph bitmap cache (16 MB by default), redundant next to
/// GPUI's atlas. Private SPI, looked up at runtime and skipped when absent.
#[cfg(target_os = "macos")]
pub fn cap_glyph_cache(bytes: usize) {
    use std::ffi::c_void;
    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
    }
    const RTLD_DEFAULT: *mut c_void = -2isize as *mut c_void;
    unsafe {
        let get = dlsym(RTLD_DEFAULT, c"CGFontCacheGetLocalCache".as_ptr());
        let set = dlsym(RTLD_DEFAULT, c"CGFontCacheSetMaxSize".as_ptr());
        if get.is_null() || set.is_null() {
            return;
        }
        let get: extern "C" fn() -> *mut c_void = std::mem::transmute(get);
        let set: extern "C" fn(*mut c_void, usize) = std::mem::transmute(set);
        let cache = get();
        if !cache.is_null() {
            set(cache, bytes);
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn cap_glyph_cache(_: usize) {}
