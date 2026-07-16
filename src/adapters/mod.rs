pub mod ace;
pub mod acr;
pub mod lmu;

#[cfg(any(windows, test))]
use crate::shared_memory;

#[cfg(windows)]
use std::thread;
#[cfg(windows)]
use std::time::{Duration, Instant};

#[cfg(windows)]
use crate::config::BridgeConfig;
#[cfg(windows)]
use crate::hud::HudHandle;
#[cfg(windows)]
use crate::logging::{TelemetryRecorder, print_enabled_outputs};
#[cfg(windows)]
use crate::telemetry::TelemetryUpdate;

#[cfg(windows)]
const POLL_INTERVAL: Duration = Duration::from_millis(50);
#[cfg(windows)]
const WARNING_INTERVAL: Duration = Duration::from_secs(2);
#[cfg(any(windows, test))]
const LMU_TELEMETRY_OFFSET: usize = 128_464;
#[cfg(any(windows, test))]
const LMU_MAX_VEHICLES: usize = 104;
#[cfg(any(windows, test))]
const LMU_STABILITY_MARKERS: [shared_memory::StabilityMarker; 3] = [
    shared_memory::StabilityMarker::new(1_700, 8),
    shared_memory::StabilityMarker::new(1_736, 4),
    shared_memory::StabilityMarker::new(LMU_TELEMETRY_OFFSET, 4),
];
#[cfg(any(windows, test))]
const LMU_PLAYER_TELEMETRY_REGION: shared_memory::IndexedStabilityRegion =
    shared_memory::IndexedStabilityRegion::new(
        LMU_TELEMETRY_OFFSET + 1,
        LMU_TELEMETRY_OFFSET + 4,
        (lmu::LMU_VIEW_SIZE - (LMU_TELEMETRY_OFFSET + 4)) / LMU_MAX_VEHICLES,
        LMU_MAX_VEHICLES,
    );
#[cfg(windows)]
const PACKET_ID_MARKER: [shared_memory::StabilityMarker; 1] =
    [shared_memory::StabilityMarker::new(0, 4)];

#[cfg(windows)]
pub(crate) enum AutoSharedMemoryRead {
    Unavailable,
    Connected(Option<TelemetryUpdate>),
}

#[cfg(windows)]
#[derive(Default)]
pub(crate) struct AutoSharedMemoryReaders {
    lmu: Option<shared_memory::SharedMemoryReader>,
    ace: Option<shared_memory::SharedMemoryReader>,
}

#[cfg(not(windows))]
#[derive(Default)]
pub(crate) struct AutoSharedMemoryReaders {
    _private: (),
}

#[cfg(windows)]
impl AutoSharedMemoryReaders {
    pub(crate) fn read_lmu_update(
        &mut self,
        frame_identifier: u32,
    ) -> Result<AutoSharedMemoryRead, String> {
        read_auto_update(
            &mut self.lmu,
            lmu::LMU_MAPPING_NAME,
            lmu::LMU_VIEW_SIZE,
            &LMU_STABILITY_MARKERS,
            |snapshot| lmu::parse_lmu_update(snapshot, frame_identifier),
        )
    }

    pub(crate) fn read_ace_update(
        &mut self,
        frame_identifier: u32,
    ) -> Result<AutoSharedMemoryRead, String> {
        read_auto_update(
            &mut self.ace,
            ace::ACE_MAPPING_NAME,
            ace::ACE_PHYSICS_MIN_SIZE,
            &PACKET_ID_MARKER,
            |snapshot| ace::parse_ace_update(snapshot, frame_identifier),
        )
    }
}

#[cfg(windows)]
fn read_auto_update<F>(
    reader: &mut Option<shared_memory::SharedMemoryReader>,
    mapping_name: &str,
    snapshot_size: usize,
    markers: &[shared_memory::StabilityMarker],
    parse: F,
) -> Result<AutoSharedMemoryRead, String>
where
    F: FnOnce(&[u8]) -> Result<Option<TelemetryUpdate>, String>,
{
    if reader.is_none() {
        let Ok(opened) = shared_memory::SharedMemoryReader::open(mapping_name, snapshot_size)
        else {
            return Ok(AutoSharedMemoryRead::Unavailable);
        };
        *reader = Some(opened);
    }

    let result = read_adapter_snapshot(
        reader
            .as_ref()
            .expect("shared-memory reader was initialized"),
        mapping_name,
        markers,
    )
    .and_then(|snapshot| parse(&snapshot));
    match result {
        Ok(update) => Ok(AutoSharedMemoryRead::Connected(update)),
        Err(error) => {
            *reader = None;
            Err(error)
        }
    }
}

#[cfg(windows)]
#[cfg_attr(windows, allow(dead_code))]
fn run_shared_memory_adapter<F>(
    config: BridgeConfig,
    adapter_name: &str,
    mapping_name: &str,
    snapshot_size: usize,
    markers: &'static [shared_memory::StabilityMarker],
    parse_update: F,
) -> Result<(), String>
where
    F: FnMut(&[u8], u32) -> Result<Option<TelemetryUpdate>, String>,
{
    run_shared_memory_adapter_loop(
        config,
        adapter_name,
        mapping_name,
        snapshot_size,
        markers,
        parse_update,
        None,
    )
}

#[cfg(windows)]
fn run_shared_memory_adapter_with_hud<F>(
    config: BridgeConfig,
    adapter_name: &str,
    mapping_name: &str,
    snapshot_size: usize,
    markers: &'static [shared_memory::StabilityMarker],
    parse_update: F,
    hud: Option<HudHandle>,
) -> Result<(), String>
where
    F: FnMut(&[u8], u32) -> Result<Option<TelemetryUpdate>, String>,
{
    run_shared_memory_adapter_loop(
        config,
        adapter_name,
        mapping_name,
        snapshot_size,
        markers,
        parse_update,
        hud,
    )
}

#[cfg(windows)]
fn run_shared_memory_adapter_loop<F>(
    config: BridgeConfig,
    adapter_name: &str,
    mapping_name: &str,
    snapshot_size: usize,
    markers: &'static [shared_memory::StabilityMarker],
    mut parse_update: F,
    hud: Option<HudHandle>,
) -> Result<(), String>
where
    F: FnMut(&[u8], u32) -> Result<Option<TelemetryUpdate>, String>,
{
    let mut recorder = TelemetryRecorder::open(&config)?;
    let mut frame_identifier = 0_u32;
    let mut warned = false;
    let mut last_warning = Instant::now();
    let mut reader = None;
    let monitor_started = Instant::now();
    let mut frame_monitor = shared_memory::TelemetryFrameMonitor::default();

    println!("{adapter_name} adapter reading {mapping_name}");
    println!("game={} ({})", config.game.id, config.game.name);
    println!("debug={}", config.debug);
    print_enabled_outputs(&config);
    print_optional_hud(hud.is_some());

    while !crate::runtime_control::shutdown_requested() {
        let snapshot = (|| {
            if reader.is_none() {
                reader = Some(shared_memory::SharedMemoryReader::open(
                    mapping_name,
                    snapshot_size,
                )?);
            }
            read_adapter_snapshot(
                reader
                    .as_ref()
                    .expect("shared-memory reader was initialized"),
                mapping_name,
                markers,
            )
        })();
        match snapshot {
            Ok(snapshot) => {
                frame_identifier = frame_identifier.wrapping_add(1);
                match parse_update(&snapshot, frame_identifier) {
                    Ok(Some(update)) => match frame_monitor
                        .observe(telemetry_frame(&update), monitor_started.elapsed())
                    {
                        Ok(shared_memory::TelemetryFrameState::Fresh)
                        | Ok(shared_memory::TelemetryFrameState::Reset) => {
                            warned = false;
                            if let Some(hud) = &hud {
                                hud.update(&update);
                            }
                            recorder.ingest(&update, config.debug);
                        }
                        Ok(shared_memory::TelemetryFrameState::Duplicate) => {
                            warned = false;
                        }
                        Err(error) => {
                            reader = None;
                            if !warned || last_warning.elapsed() >= WARNING_INTERVAL {
                                eprintln!(
                                    "[adapter-warning] {error}; rejected {adapter_name} frame"
                                );
                                warned = true;
                                last_warning = Instant::now();
                            }
                        }
                    },
                    Ok(None) => warned = false,
                    Err(error) => {
                        reader = None;
                        if !warned || last_warning.elapsed() >= WARNING_INTERVAL {
                            eprintln!(
                                "[adapter-warning] {error}; rejected {adapter_name} snapshot"
                            );
                            warned = true;
                            last_warning = Instant::now();
                        }
                    }
                }
            }
            Err(error) => {
                reader = None;
                if shared_memory::is_stalled_error(&error) {
                    // A paused/live producer and an exited producer both stop advancing markers.
                    // Reopening releases stale mappings without surfacing normal pauses as errors.
                    warned = false;
                } else if !warned || last_warning.elapsed() >= WARNING_INTERVAL {
                    eprintln!("[adapter-warning] {error}; waiting for {mapping_name}");
                    warned = true;
                    last_warning = Instant::now();
                }
            }
        }

        thread::sleep(POLL_INTERVAL);
    }
    Ok(())
}

#[cfg(windows)]
fn read_adapter_snapshot(
    reader: &shared_memory::SharedMemoryReader,
    mapping_name: &str,
    markers: &[shared_memory::StabilityMarker],
) -> Result<Vec<u8>, String> {
    if mapping_name == lmu::LMU_MAPPING_NAME {
        reader.read_consistent_indexed(markers, LMU_PLAYER_TELEMETRY_REGION)
    } else {
        reader.read_consistent(markers)
    }
}

#[cfg(windows)]
fn telemetry_frame(update: &TelemetryUpdate) -> shared_memory::TelemetryFrame {
    let session_time_s = update
        .input
        .as_ref()
        .map(|sample| f64::from(sample.session_time))
        .or_else(|| {
            update
                .lap
                .as_ref()
                .map(|sample| f64::from(sample.session_time))
        })
        .or_else(|| {
            update
                .session
                .as_ref()
                .map(|sample| f64::from(sample.session_time))
        });
    shared_memory::TelemetryFrame {
        session_time_s,
        elapsed_s: update
            .lap
            .as_ref()
            .map(|sample| f64::from(sample.current_lap_time_ms) / 1_000.0),
        lap_number: update
            .lap
            .as_ref()
            .map(|sample| i32::from(sample.current_lap_num)),
        lap_distance_m: update
            .lap
            .as_ref()
            .map(|sample| f64::from(sample.lap_distance_m)),
        track_length_m: update
            .session
            .as_ref()
            .map(|sample| f64::from(sample.track_length_m)),
        speed_kmh: update
            .input
            .as_ref()
            .map(|sample| f64::from(sample.speed_kmh)),
        rpm: update.input.as_ref().map(|sample| f64::from(sample.rpm)),
        gear: update.input.as_ref().map(|sample| i32::from(sample.gear)),
        lateral_g: None,
        longitudinal_g: None,
        throttle: update
            .input
            .as_ref()
            .map(|sample| f64::from(sample.throttle)),
        brake: update.input.as_ref().map(|sample| f64::from(sample.brake)),
        steer: update.input.as_ref().map(|sample| f64::from(sample.steer)),
        clutch: update
            .input
            .as_ref()
            .map(|sample| f64::from(sample.clutch) / 100.0),
        world_x: None,
        world_z: None,
    }
}

#[cfg(windows)]
fn print_optional_hud(enabled: bool) {
    if enabled {
        println!("HUD: native window");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lmu_consistency_covers_session_metadata_and_selected_player_block() {
        assert_eq!(LMU_STABILITY_MARKERS.len(), 3);
        assert_eq!(LMU_STABILITY_MARKERS[0].offset, 1_700);
        assert_eq!(LMU_STABILITY_MARKERS[1].offset, 1_736);
        assert_eq!(LMU_STABILITY_MARKERS[2].offset, LMU_TELEMETRY_OFFSET);
        assert_eq!(LMU_STABILITY_MARKERS[2].length, 4);
        assert_eq!(LMU_PLAYER_TELEMETRY_REGION.index_offset, 128_465);
        assert_eq!(LMU_PLAYER_TELEMETRY_REGION.blocks_offset, 128_468);
        assert_eq!(LMU_PLAYER_TELEMETRY_REGION.block_size, 1_888);
        assert_eq!(LMU_PLAYER_TELEMETRY_REGION.block_count, 104);
        assert_eq!(
            LMU_PLAYER_TELEMETRY_REGION.blocks_offset
                + LMU_PLAYER_TELEMETRY_REGION.block_size * LMU_PLAYER_TELEMETRY_REGION.block_count,
            lmu::LMU_VIEW_SIZE
        );
    }
}
