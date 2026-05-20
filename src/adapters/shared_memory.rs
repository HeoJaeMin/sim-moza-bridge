#[cfg(windows)]
use std::ffi::{OsStr, c_void};
#[cfg(windows)]
use std::io;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::slice;

#[cfg(windows)]
type Handle = *mut c_void;

#[cfg(windows)]
const FILE_MAP_READ: u32 = 0x0004;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenFileMappingW(dwDesiredAccess: u32, bInheritHandle: i32, lpName: *const u16) -> Handle;
    fn MapViewOfFile(
        hFileMappingObject: Handle,
        dwDesiredAccess: u32,
        dwFileOffsetHigh: u32,
        dwFileOffsetLow: u32,
        dwNumberOfBytesToMap: usize,
    ) -> *mut c_void;
    fn UnmapViewOfFile(lpBaseAddress: *const c_void) -> i32;
    fn CloseHandle(hObject: Handle) -> i32;
}

#[cfg(windows)]
pub fn read_mapping(name: &str, size: usize) -> Result<Vec<u8>, String> {
    let wide_name: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();

    unsafe {
        let handle = OpenFileMappingW(FILE_MAP_READ, 0, wide_name.as_ptr());
        if handle.is_null() {
            return Err(format!(
                "failed to open shared memory mapping {name}: {}",
                io::Error::last_os_error()
            ));
        }

        let view = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, size);
        if view.is_null() {
            let error = io::Error::last_os_error();
            CloseHandle(handle);
            return Err(format!("failed to map shared memory {name}: {error}"));
        }

        let bytes = slice::from_raw_parts(view.cast::<u8>(), size).to_vec();
        UnmapViewOfFile(view);
        CloseHandle(handle);
        Ok(bytes)
    }
}
