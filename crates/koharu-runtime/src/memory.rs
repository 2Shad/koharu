//! Allocator maintenance for platforms that retain freed large blocks.

/// Releases freed large allocations back to the operating system.
///
/// Android's Scudo secondary allocator keeps freed large blocks cached, so a
/// pipeline that allocates and drops multi-megabyte tensors on every page grows
/// in RSS until the OS kills the process. Disabling that cache and purging here
/// makes the mobile footprint track live data instead of allocation history.
/// Desktop allocators unmap freed large blocks already, so this is a no-op.
#[cfg(target_os = "android")]
pub fn release_freed_memory() {
    use std::os::raw::c_int;

    // bionic `malloc.h` options. Values are stable ABI; unknown options are
    // ignored by older releases, so this stays safe on API 31/32/33.
    const M_DECAY_TIME: c_int = -100;
    const M_PURGE: c_int = -101;
    const M_PURGE_ALL: c_int = -104;
    const M_CACHE_COUNT_MAX: c_int = -200;
    const M_CACHE_SIZE_MAX: c_int = -201;

    unsafe extern "C" {
        fn mallopt(param: c_int, value: c_int) -> c_int;
    }

    // SAFETY: `mallopt` only mutates allocator-internal state.
    unsafe {
        mallopt(M_CACHE_COUNT_MAX, 0);
        mallopt(M_CACHE_SIZE_MAX, 0);
        mallopt(M_DECAY_TIME, 0);
        mallopt(M_PURGE, 0);
        mallopt(M_PURGE_ALL, 0);
    }
}

#[cfg(not(target_os = "android"))]
pub fn release_freed_memory() {}
