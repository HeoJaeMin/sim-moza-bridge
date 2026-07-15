#[cfg(windows)]
use std::ffi::{OsStr, c_void};
#[cfg(windows)]
use std::io;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::slice;
#[cfg(windows)]
use std::sync::atomic::{Ordering, compiler_fence};
#[cfg(windows)]
use std::thread;

#[cfg(windows)]
type Handle = *mut c_void;

#[cfg(windows)]
const FILE_MAP_READ: u32 = 0x0004;
#[cfg(windows)]
const STABLE_READ_ATTEMPTS: usize = 5;

#[cfg(windows)]
#[derive(Clone, Copy, Debug)]
pub struct StabilityMarker {
    pub offset: usize,
    pub length: usize,
}

#[cfg(windows)]
impl StabilityMarker {
    pub const fn new(offset: usize, length: usize) -> Self {
        Self { offset, length }
    }
}

#[cfg(windows)]
pub struct SharedMemoryReader {
    name: String,
    handle: Handle,
    view: *const u8,
    size: usize,
}

// Windows mapping handles and views remain valid when a Tokio task migrates threads.
#[cfg(windows)]
unsafe impl Send for SharedMemoryReader {}

#[cfg(windows)]
impl SharedMemoryReader {
    pub fn open(name: &str, size: usize) -> Result<Self, String> {
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

            Ok(Self {
                name: name.to_owned(),
                handle,
                view: view.cast::<u8>(),
                size,
            })
        }
    }

    pub fn read_consistent(&self, markers: &[StabilityMarker]) -> Result<Vec<u8>, String> {
        validate_markers(self.size, markers)?;
        for _ in 0..STABLE_READ_ATTEMPTS {
            let before = self.read_markers(markers);
            compiler_fence(Ordering::Acquire);
            let snapshot = unsafe { slice::from_raw_parts(self.view, self.size).to_vec() };
            compiler_fence(Ordering::Acquire);
            let after = self.read_markers(markers);
            if before == after && snapshot_markers_match(&snapshot, markers, &before) {
                return Ok(snapshot);
            }
            thread::yield_now();
        }
        Err(format!(
            "shared memory mapping {} changed during {} consecutive reads",
            self.name, STABLE_READ_ATTEMPTS
        ))
    }

    fn read_markers(&self, markers: &[StabilityMarker]) -> Vec<u8> {
        let length = markers.iter().map(|marker| marker.length).sum();
        let mut values = Vec::with_capacity(length);
        for marker in markers {
            for byte in 0..marker.length {
                values.push(unsafe { self.view.add(marker.offset + byte).read_volatile() });
            }
        }
        values
    }
}

#[cfg(windows)]
impl Drop for SharedMemoryReader {
    fn drop(&mut self) {
        unsafe {
            UnmapViewOfFile(self.view.cast::<c_void>());
            CloseHandle(self.handle);
        }
    }
}

#[cfg(windows)]
pub fn read_mapping(name: &str, size: usize) -> Result<Vec<u8>, String> {
    SharedMemoryReader::open(name, size)?.read_consistent(&[])
}

#[cfg(windows)]
pub fn mapping_exists(name: &str) -> bool {
    let wide_name: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();

    unsafe {
        let handle = OpenFileMappingW(FILE_MAP_READ, 0, wide_name.as_ptr());
        if handle.is_null() {
            false
        } else {
            CloseHandle(handle);
            true
        }
    }
}

#[cfg(windows)]
fn validate_markers(size: usize, markers: &[StabilityMarker]) -> Result<(), String> {
    if let Some(marker) = markers
        .iter()
        .find(|marker| marker.length == 0 || marker.offset.saturating_add(marker.length) > size)
    {
        return Err(format!(
            "invalid shared-memory stability marker {}..{} for {size}-byte mapping",
            marker.offset,
            marker.offset.saturating_add(marker.length)
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn snapshot_markers_match(snapshot: &[u8], markers: &[StabilityMarker], expected: &[u8]) -> bool {
    let mut cursor = 0;
    for marker in markers {
        let Some(bytes) = snapshot.get(marker.offset..marker.offset + marker.length) else {
            return false;
        };
        if expected.get(cursor..cursor + marker.length) != Some(bytes) {
            return false;
        }
        cursor += marker.length;
    }
    cursor == expected.len()
}

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

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn compares_multiple_stability_markers() {
        let snapshot = [1, 2, 3, 4, 5, 6, 7, 8];
        let markers = [StabilityMarker::new(1, 2), StabilityMarker::new(6, 2)];
        assert!(snapshot_markers_match(&snapshot, &markers, &[2, 3, 7, 8]));
        assert!(!snapshot_markers_match(&snapshot, &markers, &[2, 4, 7, 8]));
    }

    #[test]
    fn rejects_out_of_bounds_markers() {
        let error = validate_markers(8, &[StabilityMarker::new(7, 2)]).unwrap_err();
        assert!(error.contains("invalid shared-memory stability marker"));
    }
}
