#![cfg_attr(not(any(windows, test)), allow(dead_code))]

use std::sync::atomic::{Ordering, compiler_fence};
use std::thread;
use std::time::Duration;

#[cfg(windows)]
use std::ffi::{OsStr, c_void};
#[cfg(windows)]
use std::io;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::slice;
#[cfg(windows)]
use std::sync::Mutex;
#[cfg(windows)]
use std::time::Instant;

const STABLE_READ_ATTEMPTS: usize = 5;
const DEFAULT_STALE_AFTER: Duration = Duration::from_secs(3);
const STALLED_MAPPING_PREFIX: &str = "stalled shared-memory mapping";
const MAX_SPEED_KMH: f64 = 650.0;
const MAX_RPM: f64 = 30_000.0;
const MAX_TRACK_LENGTH_M: f64 = 100_000.0;
const MAX_ABSOLUTE_G: f64 = 20.0;

#[derive(Clone, Copy, Debug)]
pub struct StabilityMarker {
    pub offset: usize,
    pub length: usize,
}

impl StabilityMarker {
    pub const fn new(offset: usize, length: usize) -> Self {
        Self { offset, length }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct IndexedStabilityRegion {
    pub index_offset: usize,
    pub blocks_offset: usize,
    pub block_size: usize,
    pub block_count: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct CountedStabilityRegion {
    pub count_offset: usize,
    pub blocks_offset: usize,
    pub block_size: usize,
    pub max_block_count: usize,
}

impl CountedStabilityRegion {
    pub const fn new(
        count_offset: usize,
        blocks_offset: usize,
        block_size: usize,
        max_block_count: usize,
    ) -> Self {
        Self {
            count_offset,
            blocks_offset,
            block_size,
            max_block_count,
        }
    }
}

impl IndexedStabilityRegion {
    pub const fn new(
        index_offset: usize,
        blocks_offset: usize,
        block_size: usize,
        block_count: usize,
    ) -> Self {
        Self {
            index_offset,
            blocks_offset,
            block_size,
            block_count,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SnapshotStats {
    pub fresh_frames: u64,
    pub duplicate_frames: u64,
    pub stalled_frames: u64,
    pub inconsistent_reads: u64,
    pub untracked_frames: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FreshnessState {
    Fresh,
    Duplicate,
    Untracked,
}

#[derive(Debug)]
pub struct FrameFreshness {
    stale_after: Duration,
    last_signature: Option<Vec<u8>>,
    last_change_at: Duration,
    stats: SnapshotStats,
}

impl FrameFreshness {
    pub fn new(stale_after: Duration) -> Self {
        Self {
            stale_after,
            last_signature: None,
            last_change_at: Duration::ZERO,
            stats: SnapshotStats::default(),
        }
    }

    pub fn observe(&mut self, signature: &[u8], now: Duration) -> Result<FreshnessState, String> {
        if self.last_signature.as_deref() != Some(signature) {
            self.last_signature = Some(signature.to_vec());
            self.last_change_at = now;
            self.stats.fresh_frames = self.stats.fresh_frames.saturating_add(1);
            return Ok(FreshnessState::Fresh);
        }

        self.stats.duplicate_frames = self.stats.duplicate_frames.saturating_add(1);
        if now.saturating_sub(self.last_change_at) >= self.stale_after {
            self.stats.stalled_frames = self.stats.stalled_frames.saturating_add(1);
            return Err(format!(
                "{STALLED_MAPPING_PREFIX}: markers have not advanced for {:.1}s",
                self.stale_after.as_secs_f64()
            ));
        }
        Ok(FreshnessState::Duplicate)
    }

    pub fn stats(&self) -> SnapshotStats {
        self.stats
    }

    fn record_untracked_read(&mut self) {
        self.stats.untracked_frames = self.stats.untracked_frames.saturating_add(1);
    }

    #[cfg(windows)]
    fn record_inconsistent_read(&mut self) {
        self.stats.inconsistent_reads = self.stats.inconsistent_reads.saturating_add(1);
    }
}

pub fn is_stalled_error(error: &str) -> bool {
    error.starts_with(STALLED_MAPPING_PREFIX)
}

fn classify_snapshot(
    freshness: &mut FrameFreshness,
    markers: &[StabilityMarker],
    signature: &[u8],
    now: Duration,
) -> Result<FreshnessState, String> {
    if markers.is_empty() {
        freshness.record_untracked_read();
        Ok(FreshnessState::Untracked)
    } else {
        freshness.observe(signature, now)
    }
}

impl Default for FrameFreshness {
    fn default() -> Self {
        Self::new(DEFAULT_STALE_AFTER)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TelemetryFrame {
    pub session_time_s: Option<f64>,
    pub elapsed_s: Option<f64>,
    pub lap_number: Option<i32>,
    pub lap_distance_m: Option<f64>,
    pub track_length_m: Option<f64>,
    pub speed_kmh: Option<f64>,
    pub rpm: Option<f64>,
    pub gear: Option<i32>,
    pub lateral_g: Option<f64>,
    pub longitudinal_g: Option<f64>,
    pub throttle: Option<f64>,
    pub brake: Option<f64>,
    pub steer: Option<f64>,
    pub clutch: Option<f64>,
    pub world_x: Option<f64>,
    pub world_z: Option<f64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TelemetryFrameState {
    Fresh,
    Duplicate,
    Reset,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TelemetryStreamStats {
    pub accepted_frames: u64,
    pub duplicate_frames: u64,
    pub backward_frames: u64,
    pub delayed_frames: u64,
    pub rejected_frames: u64,
    pub sudden_change_frames: u64,
}

#[derive(Debug, Default)]
pub struct TelemetryFrameMonitor {
    previous: Option<(TelemetryFrame, Duration)>,
    stats: TelemetryStreamStats,
}

impl TelemetryFrameMonitor {
    pub fn observe(
        &mut self,
        frame: TelemetryFrame,
        now: Duration,
    ) -> Result<TelemetryFrameState, String> {
        if let Err(error) = validate_frame(frame) {
            self.stats.rejected_frames = self.stats.rejected_frames.saturating_add(1);
            return Err(error);
        }

        let Some((previous, previous_at)) = self.previous else {
            return Ok(self.accept(frame, now, TelemetryFrameState::Fresh));
        };

        if frame == previous {
            self.stats.duplicate_frames = self.stats.duplicate_frames.saturating_add(1);
            return Ok(TelemetryFrameState::Duplicate);
        }

        let wall_delta_s = now.saturating_sub(previous_at).as_secs_f64();
        let session_delta_s = match (frame.session_time_s, previous.session_time_s) {
            (Some(current), Some(last)) => Some(current - last),
            _ => None,
        };
        if let Some(delta) = session_delta_s {
            if delta < -5.0 {
                self.stats.backward_frames = self.stats.backward_frames.saturating_add(1);
                return Ok(self.accept(frame, now, TelemetryFrameState::Reset));
            }
            if delta < -0.05 {
                return self.reject_backward("session time");
            }
            if wall_delta_s >= 1.0 && delta >= 0.0 && delta < wall_delta_s * 0.25 {
                self.stats.delayed_frames = self.stats.delayed_frames.saturating_add(1);
            }
        }

        if let (Some(current_lap), Some(previous_lap)) = (frame.lap_number, previous.lap_number)
            && current_lap < previous_lap
        {
            self.stats.backward_frames = self.stats.backward_frames.saturating_add(1);
            return Ok(self.accept(frame, now, TelemetryFrameState::Reset));
        }

        if let (Some(current_speed), Some(previous_speed)) = (frame.speed_kmh, previous.speed_kmh) {
            let elapsed = session_delta_s
                .filter(|delta| *delta > 0.0)
                .unwrap_or(wall_delta_s);
            if elapsed <= 0.25 && (current_speed - previous_speed).abs() > 160.0 {
                return self.reject_sudden(format!(
                    "rejected sudden speed change {:.1}->{current_speed:.1} km/h in {elapsed:.3}s",
                    previous_speed
                ));
            }
        }

        if let (Some(current_distance), Some(previous_distance)) =
            (frame.lap_distance_m, previous.lap_distance_m)
        {
            let same_lap = match (frame.lap_number, previous.lap_number) {
                (Some(current), Some(last)) => current == last,
                _ => true,
            };
            if same_lap {
                let track_length = frame.track_length_m.or(previous.track_length_m);
                let wrapped = track_length.is_some_and(|length| {
                    previous_distance >= length * 0.9 && current_distance <= length * 0.1
                });
                let backwards_tolerance =
                    track_length.map_or(5.0, |length| (length * 0.002).clamp(5.0, 30.0));
                if previous_distance - current_distance > backwards_tolerance && !wrapped {
                    return self.reject_backward("lap distance");
                }

                let forward = current_distance - previous_distance;
                if forward > 0.0 {
                    let elapsed = session_delta_s
                        .filter(|delta| *delta > 0.0)
                        .unwrap_or(wall_delta_s)
                        .max(0.0);
                    let speed_kmh = frame
                        .speed_kmh
                        .unwrap_or_default()
                        .max(previous.speed_kmh.unwrap_or_default());
                    let allowed = (speed_kmh / 3.6 * elapsed * 3.0 + 50.0).max(100.0);
                    if forward > allowed {
                        return self.reject_sudden(
                            format!(
                                "rejected impossible forward distance jump {forward:.1}m in {elapsed:.3}s"
                            ),
                        );
                    }
                }
            }
        }

        Ok(self.accept(frame, now, TelemetryFrameState::Fresh))
    }

    pub fn stats(&self) -> TelemetryStreamStats {
        self.stats
    }

    fn accept(
        &mut self,
        frame: TelemetryFrame,
        now: Duration,
        state: TelemetryFrameState,
    ) -> TelemetryFrameState {
        self.previous = Some((frame, now));
        self.stats.accepted_frames = self.stats.accepted_frames.saturating_add(1);
        state
    }

    fn reject_backward<T>(&mut self, field: &str) -> Result<T, String> {
        self.stats.backward_frames = self.stats.backward_frames.saturating_add(1);
        self.stats.rejected_frames = self.stats.rejected_frames.saturating_add(1);
        Err(format!("rejected backward {field}"))
    }

    fn reject_sudden<T>(&mut self, message: String) -> Result<T, String> {
        self.stats.sudden_change_frames = self.stats.sudden_change_frames.saturating_add(1);
        self.stats.rejected_frames = self.stats.rejected_frames.saturating_add(1);
        Err(message)
    }
}

fn validate_frame(frame: TelemetryFrame) -> Result<(), String> {
    validate_range("session time", frame.session_time_s, 0.0, f64::MAX)?;
    validate_range("elapsed time", frame.elapsed_s, 0.0, f64::MAX)?;
    validate_range("speed", frame.speed_kmh, 0.0, MAX_SPEED_KMH)?;
    validate_range("RPM", frame.rpm, 0.0, MAX_RPM)?;
    validate_range(
        "lateral G",
        frame.lateral_g,
        -MAX_ABSOLUTE_G,
        MAX_ABSOLUTE_G,
    )?;
    validate_range(
        "longitudinal G",
        frame.longitudinal_g,
        -MAX_ABSOLUTE_G,
        MAX_ABSOLUTE_G,
    )?;
    validate_range("throttle", frame.throttle, 0.0, 1.0)?;
    validate_range("brake", frame.brake, 0.0, 1.0)?;
    validate_range("steer", frame.steer, -1.0, 1.0)?;
    validate_range("clutch", frame.clutch, 0.0, 1.0)?;
    validate_range("world X", frame.world_x, -1_000_000.0, 1_000_000.0)?;
    validate_range("world Z", frame.world_z, -1_000_000.0, 1_000_000.0)?;
    validate_range(
        "track length",
        frame.track_length_m,
        0.0,
        MAX_TRACK_LENGTH_M,
    )?;
    if let Some(gear) = frame.gear
        && !(-1..=12).contains(&gear)
    {
        return Err(format!("rejected impossible gear {gear}"));
    }
    if let Some(distance) = frame.lap_distance_m {
        if !distance.is_finite() || distance < -50.0 {
            return Err(format!("rejected impossible lap distance {distance}"));
        }
        let maximum = frame
            .track_length_m
            .filter(|length| *length >= 100.0)
            .map_or(MAX_TRACK_LENGTH_M, |length| length * 1.05 + 50.0);
        if distance > maximum {
            return Err(format!(
                "rejected lap distance {distance:.1}m beyond {maximum:.1}m envelope"
            ));
        }
    }
    Ok(())
}

fn validate_range(
    name: &str,
    value: Option<f64>,
    minimum: f64,
    maximum: f64,
) -> Result<(), String> {
    if let Some(value) = value
        && (!value.is_finite() || value < minimum || value > maximum)
    {
        return Err(format!("rejected impossible {name} {value}"));
    }
    Ok(())
}

trait SnapshotSource {
    fn len(&self) -> usize;
    fn read_byte(&self, offset: usize) -> u8;
    fn copy_bytes(&self, destination: &mut [u8]);
}

fn copy_consistent<S: SnapshotSource>(
    source: &S,
    name: &str,
    size: usize,
    markers: &[StabilityMarker],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    if source.len() < size {
        return Err(format!(
            "shared memory mapping {name} is shorter than requested: {} < {size}",
            source.len()
        ));
    }
    validate_markers(size, markers)?;
    if markers.is_empty() {
        for _ in 0..STABLE_READ_ATTEMPTS {
            let mut first = vec![0; size];
            source.copy_bytes(&mut first);
            compiler_fence(Ordering::Acquire);
            let mut second = vec![0; size];
            source.copy_bytes(&mut second);
            if first == second {
                return Ok((second, Vec::new()));
            }
            thread::yield_now();
        }
        return Err(format!(
            "shared memory mapping {name} changed during {STABLE_READ_ATTEMPTS} consecutive reads"
        ));
    }
    for _ in 0..STABLE_READ_ATTEMPTS {
        let before = read_markers(source, markers);
        compiler_fence(Ordering::Acquire);
        let mut snapshot = vec![0; size];
        source.copy_bytes(&mut snapshot);
        compiler_fence(Ordering::Acquire);
        let after = read_markers(source, markers);
        if before == after && snapshot_markers_match(&snapshot, markers, &before) {
            return Ok((snapshot, before));
        }
        thread::yield_now();
    }
    Err(format!(
        "shared memory mapping {name} changed during {STABLE_READ_ATTEMPTS} consecutive reads"
    ))
}

fn copy_consistent_indexed<S: SnapshotSource>(
    source: &S,
    name: &str,
    size: usize,
    markers: &[StabilityMarker],
    indexed: IndexedStabilityRegion,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    if source.len() < size {
        return Err(format!(
            "shared memory mapping {name} is shorter than requested: {} < {size}",
            source.len()
        ));
    }
    validate_markers(size, markers)?;
    validate_indexed_region(size, indexed)?;
    for _ in 0..STABLE_READ_ATTEMPTS {
        let index = source.read_byte(indexed.index_offset) as usize;
        if index >= indexed.block_count {
            thread::yield_now();
            continue;
        }
        let block = StabilityMarker::new(
            indexed.blocks_offset + index * indexed.block_size,
            indexed.block_size,
        );
        let mut current_markers = Vec::with_capacity(markers.len() + 2);
        current_markers.extend_from_slice(markers);
        current_markers.push(StabilityMarker::new(indexed.index_offset, 1));
        current_markers.push(block);

        let before = read_markers(source, &current_markers);
        compiler_fence(Ordering::Acquire);
        let mut snapshot = vec![0; size];
        source.copy_bytes(&mut snapshot);
        compiler_fence(Ordering::Acquire);
        let after = read_markers(source, &current_markers);
        if snapshot[indexed.index_offset] as usize == index
            && before == after
            && snapshot_markers_match(&snapshot, &current_markers, &before)
        {
            return Ok((snapshot, before));
        }
        thread::yield_now();
    }
    Err(format!(
        "shared memory mapping {name} changed during {STABLE_READ_ATTEMPTS} consecutive reads"
    ))
}

fn copy_consistent_counted<S: SnapshotSource>(
    source: &S,
    name: &str,
    size: usize,
    markers: &[StabilityMarker],
    counted_regions: &[CountedStabilityRegion],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    if source.len() < size {
        return Err(format!(
            "shared memory mapping {name} is shorter than requested: {} < {size}",
            source.len()
        ));
    }
    validate_markers(size, markers)?;
    for counted in counted_regions {
        validate_counted_region(size, *counted)?;
    }
    for _ in 0..STABLE_READ_ATTEMPTS {
        let counts = counted_regions
            .iter()
            .map(|counted| source.read_byte(counted.count_offset) as usize)
            .collect::<Vec<_>>();
        if counted_regions
            .iter()
            .zip(&counts)
            .any(|(counted, count)| *count > counted.max_block_count)
        {
            thread::yield_now();
            continue;
        }
        let mut current_markers = Vec::with_capacity(markers.len() + counted_regions.len() * 2);
        current_markers.extend_from_slice(markers);
        for (counted, count) in counted_regions.iter().zip(&counts) {
            current_markers.push(StabilityMarker::new(counted.count_offset, 1));
            if *count > 0 {
                current_markers.push(StabilityMarker::new(
                    counted.blocks_offset,
                    count * counted.block_size,
                ));
            }
        }

        let before = read_markers(source, &current_markers);
        compiler_fence(Ordering::Acquire);
        let mut snapshot = vec![0; size];
        source.copy_bytes(&mut snapshot);
        compiler_fence(Ordering::Acquire);
        let after = read_markers(source, &current_markers);
        if counted_regions
            .iter()
            .zip(&counts)
            .all(|(counted, count)| snapshot[counted.count_offset] as usize == *count)
            && before == after
            && snapshot_markers_match(&snapshot, &current_markers, &before)
        {
            return Ok((snapshot, before));
        }
        thread::yield_now();
    }
    Err(format!(
        "shared memory mapping {name} changed during {STABLE_READ_ATTEMPTS} consecutive reads"
    ))
}

fn read_markers(source: &impl SnapshotSource, markers: &[StabilityMarker]) -> Vec<u8> {
    let length = markers.iter().map(|marker| marker.length).sum();
    let mut values = Vec::with_capacity(length);
    for marker in markers {
        for byte in 0..marker.length {
            values.push(source.read_byte(marker.offset + byte));
        }
    }
    values
}

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

fn validate_indexed_region(size: usize, indexed: IndexedStabilityRegion) -> Result<(), String> {
    let blocks_length = indexed
        .block_size
        .checked_mul(indexed.block_count)
        .and_then(|length| indexed.blocks_offset.checked_add(length));
    if indexed.index_offset >= size
        || indexed.block_size == 0
        || indexed.block_count == 0
        || blocks_length.is_none_or(|end| end > size)
    {
        return Err(format!(
            "invalid indexed shared-memory region index={} blocks={}..{} for {size}-byte mapping",
            indexed.index_offset,
            indexed.blocks_offset,
            blocks_length.unwrap_or(usize::MAX)
        ));
    }
    Ok(())
}

fn validate_counted_region(size: usize, counted: CountedStabilityRegion) -> Result<(), String> {
    let blocks_end = counted
        .block_size
        .checked_mul(counted.max_block_count)
        .and_then(|length| counted.blocks_offset.checked_add(length));
    if counted.count_offset >= size
        || counted.block_size == 0
        || counted.max_block_count == 0
        || blocks_end.is_none_or(|end| end > size)
    {
        return Err(format!(
            "invalid counted shared-memory region count={} blocks={}..{} for {size}-byte mapping",
            counted.count_offset,
            counted.blocks_offset,
            blocks_end.unwrap_or(usize::MAX)
        ));
    }
    Ok(())
}

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
type Handle = *mut c_void;

#[cfg(windows)]
const FILE_MAP_READ: u32 = 0x0004;

#[cfg(windows)]
pub struct SharedMemoryReader {
    name: String,
    handle: Handle,
    view: *const u8,
    size: usize,
    opened_at: Instant,
    freshness: Mutex<FrameFreshness>,
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
                opened_at: Instant::now(),
                freshness: Mutex::new(FrameFreshness::default()),
            })
        }
    }

    pub fn read_consistent(&self, markers: &[StabilityMarker]) -> Result<Vec<u8>, String> {
        let result = copy_consistent(self, &self.name, self.size, markers);
        let (snapshot, signature) = match result {
            Ok(result) => result,
            Err(error) => {
                if let Ok(mut freshness) = self.freshness.lock() {
                    freshness.record_inconsistent_read();
                }
                return Err(error);
            }
        };
        let mut freshness = self
            .freshness
            .lock()
            .map_err(|_| format!("shared-memory freshness state poisoned for {}", self.name))?;
        classify_snapshot(
            &mut freshness,
            markers,
            &signature,
            self.opened_at.elapsed(),
        )?;
        Ok(snapshot)
    }

    pub fn read_consistent_indexed(
        &self,
        markers: &[StabilityMarker],
        indexed: IndexedStabilityRegion,
    ) -> Result<Vec<u8>, String> {
        let result = copy_consistent_indexed(self, &self.name, self.size, markers, indexed);
        let (snapshot, signature) = match result {
            Ok(result) => result,
            Err(error) => {
                if let Ok(mut freshness) = self.freshness.lock() {
                    freshness.record_inconsistent_read();
                }
                return Err(error);
            }
        };
        let mut freshness = self
            .freshness
            .lock()
            .map_err(|_| format!("shared-memory freshness state poisoned for {}", self.name))?;
        freshness.observe(&signature, self.opened_at.elapsed())?;
        Ok(snapshot)
    }

    pub fn read_consistent_counted(
        &self,
        markers: &[StabilityMarker],
        counted_regions: &[CountedStabilityRegion],
    ) -> Result<Vec<u8>, String> {
        let result = copy_consistent_counted(self, &self.name, self.size, markers, counted_regions);
        let (snapshot, signature) = match result {
            Ok(result) => result,
            Err(error) => {
                if let Ok(mut freshness) = self.freshness.lock() {
                    freshness.record_inconsistent_read();
                }
                return Err(error);
            }
        };
        let mut freshness = self
            .freshness
            .lock()
            .map_err(|_| format!("shared-memory freshness state poisoned for {}", self.name))?;
        freshness.observe(&signature, self.opened_at.elapsed())?;
        Ok(snapshot)
    }

    pub fn stats(&self) -> SnapshotStats {
        self.freshness
            .lock()
            .map_or_else(|_| SnapshotStats::default(), |freshness| freshness.stats())
    }
}

#[cfg(windows)]
impl SnapshotSource for SharedMemoryReader {
    fn len(&self) -> usize {
        self.size
    }

    fn read_byte(&self, offset: usize) -> u8 {
        unsafe { self.view.add(offset).read_volatile() }
    }

    fn copy_bytes(&self, destination: &mut [u8]) {
        let bytes = unsafe { slice::from_raw_parts(self.view, destination.len()) };
        destination.copy_from_slice(bytes);
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

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    struct FakeSource {
        bytes: RefCell<Vec<u8>>,
        replacement_after_copy: RefCell<Option<Vec<u8>>>,
    }

    impl FakeSource {
        fn new(bytes: Vec<u8>) -> Self {
            Self {
                bytes: RefCell::new(bytes),
                replacement_after_copy: RefCell::new(None),
            }
        }

        fn replace_after_copy(&self, replacement: Vec<u8>) {
            *self.replacement_after_copy.borrow_mut() = Some(replacement);
        }
    }

    impl SnapshotSource for FakeSource {
        fn len(&self) -> usize {
            self.bytes.borrow().len()
        }

        fn read_byte(&self, offset: usize) -> u8 {
            self.bytes.borrow()[offset]
        }

        fn copy_bytes(&self, destination: &mut [u8]) {
            destination.copy_from_slice(&self.bytes.borrow()[..destination.len()]);
            if let Some(replacement) = self.replacement_after_copy.borrow_mut().take() {
                *self.bytes.borrow_mut() = replacement;
            }
        }
    }

    struct AlwaysChangingSource {
        bytes: RefCell<Vec<u8>>,
        copy_count: Cell<usize>,
    }

    impl AlwaysChangingSource {
        fn new(bytes: Vec<u8>) -> Self {
            Self {
                bytes: RefCell::new(bytes),
                copy_count: Cell::new(0),
            }
        }
    }

    impl SnapshotSource for AlwaysChangingSource {
        fn len(&self) -> usize {
            self.bytes.borrow().len()
        }

        fn read_byte(&self, offset: usize) -> u8 {
            self.bytes.borrow()[offset]
        }

        fn copy_bytes(&self, destination: &mut [u8]) {
            let mut bytes = self.bytes.borrow_mut();
            destination.copy_from_slice(&bytes[..destination.len()]);
            bytes[0] = bytes[0].wrapping_add(1);
            self.copy_count.set(self.copy_count.get() + 1);
        }
    }

    #[test]
    fn retries_when_a_fake_mapping_changes_during_copy() {
        let source = FakeSource::new(vec![1, 2, 3, 4]);
        source.replace_after_copy(vec![9, 2, 3, 4]);

        let (snapshot, marker) =
            copy_consistent(&source, "fake", 4, &[StabilityMarker::new(0, 1)]).unwrap();

        assert_eq!(snapshot, vec![9, 2, 3, 4]);
        assert_eq!(marker, vec![9]);
    }

    #[test]
    fn rejects_when_a_fake_mapping_changes_during_every_copy_attempt() {
        let source = AlwaysChangingSource::new(vec![1, 2, 3, 4]);

        let error = copy_consistent(&source, "fake", 4, &[StabilityMarker::new(0, 1)]).unwrap_err();

        assert!(error.contains("changed during"));
        assert_eq!(source.copy_count.get(), STABLE_READ_ATTEMPTS);
    }

    #[test]
    fn empty_markers_require_two_matching_full_copies() {
        let source = FakeSource::new(vec![1, 2, 3, 4]);
        source.replace_after_copy(vec![9, 8, 7, 6]);

        let (snapshot, signature) = copy_consistent(&source, "fake", 4, &[]).unwrap();

        assert_eq!(snapshot, vec![9, 8, 7, 6]);
        assert!(signature.is_empty());
    }

    #[test]
    fn indexed_consistency_retries_when_the_selected_block_changes() {
        let mut initial = vec![0; 20];
        initial[0] = 1;
        initial[1] = 7;
        initial[8..12].copy_from_slice(&[1, 2, 3, 4]);
        let source = FakeSource::new(initial.clone());
        let mut replacement = initial;
        replacement[8..12].copy_from_slice(&[9, 8, 7, 6]);
        source.replace_after_copy(replacement.clone());

        let (snapshot, _) = copy_consistent_indexed(
            &source,
            "fake",
            20,
            &[StabilityMarker::new(1, 1)],
            IndexedStabilityRegion::new(0, 4, 4, 4),
        )
        .unwrap();

        assert_eq!(snapshot, replacement);
    }

    #[test]
    fn counted_consistency_retries_when_a_non_player_active_block_changes() {
        let mut initial = vec![0; 20];
        initial[0] = 2;
        initial[1] = 1;
        initial[8..12].copy_from_slice(&[1, 2, 3, 4]);
        let source = FakeSource::new(initial.clone());
        let mut replacement = initial;
        replacement[8..12].copy_from_slice(&[9, 8, 7, 6]);
        source.replace_after_copy(replacement.clone());

        let (snapshot, _) = copy_consistent_counted(
            &source,
            "fake",
            20,
            &[StabilityMarker::new(0, 2)],
            &[CountedStabilityRegion::new(0, 4, 4, 4)],
        )
        .unwrap();

        assert_eq!(snapshot, replacement);
    }

    #[test]
    fn counted_consistency_retries_when_a_non_player_scoring_block_changes() {
        let mut initial = vec![0; 40];
        initial[0] = 2;
        initial[16] = 2;
        initial[8..12].copy_from_slice(&[1, 2, 3, 4]);
        initial[24..28].copy_from_slice(&[5, 6, 7, 8]);
        let source = FakeSource::new(initial.clone());
        let mut replacement = initial;
        replacement[8..12].copy_from_slice(&[9, 10, 11, 12]);
        source.replace_after_copy(replacement.clone());

        let (snapshot, _) = copy_consistent_counted(
            &source,
            "fake",
            40,
            &[StabilityMarker::new(32, 1)],
            &[
                CountedStabilityRegion::new(0, 4, 4, 2),
                CountedStabilityRegion::new(16, 20, 4, 2),
            ],
        )
        .unwrap();

        assert_eq!(snapshot, replacement);
    }

    #[test]
    fn marks_unchanged_mapping_as_stalled() {
        let mut freshness = FrameFreshness::new(Duration::from_secs(2));

        assert_eq!(
            freshness.observe(&[1], Duration::ZERO).unwrap(),
            FreshnessState::Fresh
        );
        assert_eq!(
            freshness.observe(&[1], Duration::from_secs(1)).unwrap(),
            FreshnessState::Duplicate
        );
        let error = freshness.observe(&[1], Duration::from_secs(2)).unwrap_err();
        assert!(error.contains("have not advanced"));
        assert!(is_stalled_error(&error));
        assert_eq!(freshness.stats().duplicate_frames, 2);
        assert_eq!(freshness.stats().stalled_frames, 1);
    }

    #[test]
    fn empty_markers_are_explicitly_untracked_and_never_stalled() {
        let mut freshness = FrameFreshness::new(Duration::ZERO);

        assert_eq!(
            classify_snapshot(&mut freshness, &[], &[], Duration::from_secs(60)).unwrap(),
            FreshnessState::Untracked
        );
        assert_eq!(freshness.stats().untracked_frames, 1);
        assert_eq!(freshness.stats().stalled_frames, 0);
    }

    #[test]
    fn rejects_out_of_bounds_markers() {
        let error = validate_markers(8, &[StabilityMarker::new(7, 2)]).unwrap_err();
        assert!(error.contains("invalid shared-memory stability marker"));
    }

    #[test]
    fn rejects_impossible_distance_and_forward_spikes() {
        let mut monitor = TelemetryFrameMonitor::default();
        let base = TelemetryFrame {
            session_time_s: Some(1.0),
            elapsed_s: Some(1.0),
            lap_number: Some(1),
            lap_distance_m: Some(100.0),
            track_length_m: Some(1_000.0),
            speed_kmh: Some(100.0),
            rpm: Some(5_000.0),
            gear: Some(3),
            lateral_g: Some(0.5),
            longitudinal_g: Some(-0.5),
            throttle: Some(0.5),
            brake: Some(0.0),
            steer: Some(0.0),
            clutch: Some(0.0),
            world_x: Some(100.0),
            world_z: Some(-100.0),
        };
        monitor.observe(base, Duration::ZERO).unwrap();

        assert!(
            monitor
                .observe(
                    TelemetryFrame {
                        session_time_s: Some(1.05),
                        lap_distance_m: Some(2_000.0),
                        ..base
                    },
                    Duration::from_millis(50),
                )
                .unwrap_err()
                .contains("beyond")
        );
        assert!(
            monitor
                .observe(
                    TelemetryFrame {
                        session_time_s: Some(1.10),
                        lap_distance_m: Some(600.0),
                        ..base
                    },
                    Duration::from_millis(100),
                )
                .unwrap_err()
                .contains("forward distance jump")
        );

        assert!(
            TelemetryFrameMonitor::default()
                .observe(
                    TelemetryFrame {
                        lateral_g: Some(25.0),
                        ..base
                    },
                    Duration::ZERO,
                )
                .unwrap_err()
                .contains("lateral G")
        );
    }

    #[test]
    fn tracks_duplicates_backward_time_and_delayed_frames() {
        let mut monitor = TelemetryFrameMonitor::default();
        let base = TelemetryFrame {
            session_time_s: Some(10.0),
            speed_kmh: Some(100.0),
            rpm: Some(5_000.0),
            gear: Some(3),
            ..TelemetryFrame::default()
        };
        monitor.observe(base, Duration::ZERO).unwrap();
        assert_eq!(
            monitor.observe(base, Duration::from_millis(50)).unwrap(),
            TelemetryFrameState::Duplicate
        );
        monitor
            .observe(
                TelemetryFrame {
                    session_time_s: Some(10.1),
                    speed_kmh: Some(101.0),
                    ..base
                },
                Duration::from_secs(2),
            )
            .unwrap();
        assert!(
            monitor
                .observe(
                    TelemetryFrame {
                        session_time_s: Some(9.5),
                        speed_kmh: Some(102.0),
                        ..base
                    },
                    Duration::from_millis(2_100),
                )
                .unwrap_err()
                .contains("backward session time")
        );
        assert!(
            monitor
                .observe(
                    TelemetryFrame {
                        session_time_s: Some(9.6),
                        speed_kmh: Some(102.0),
                        ..base
                    },
                    Duration::from_millis(2_150),
                )
                .unwrap_err()
                .contains("backward session time")
        );
        assert_eq!(
            monitor
                .observe(
                    TelemetryFrame {
                        session_time_s: Some(10.2),
                        speed_kmh: Some(102.0),
                        ..base
                    },
                    Duration::from_millis(2_200),
                )
                .unwrap(),
            TelemetryFrameState::Fresh
        );

        let stats = monitor.stats();
        assert_eq!(stats.duplicate_frames, 1);
        assert_eq!(stats.backward_frames, 2);
        assert_eq!(stats.delayed_frames, 1);
    }
}
