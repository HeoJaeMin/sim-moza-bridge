use std::collections::HashMap;
use std::fs::{OpenOptions, metadata};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use serde::{Deserialize, Serialize};

use crate::analysis::{CompletedLapAnalysis, SetupRecommendation};
use crate::config::BridgeConfig;
use crate::telemetry::{
    CarSetupSample, DamageSample, FinalClassificationSample, InputSample, LapSample,
    RaceOrderCarSample, RaceOrderSample, SessionSample, StatusSample, TelemetryUpdate, TyreSetInfo,
    TyreSetsSample, WheelValuesF32,
};

// Keep the implementation private while separating the state machine, strategy,
// persistence, and voice responsibilities into reviewable source files.
include!("race_engineer/core.rs");
include!("race_engineer/strategy.rs");
include!("race_engineer/io.rs");
include!("race_engineer/voice.rs");
include!("race_engineer/runtime.rs");

#[cfg(test)]
mod tests {
    include!("race_engineer/tests/scenarios.rs");
    include!("race_engineer/tests/runtime.rs");
}
