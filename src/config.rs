use std::env;
use std::fmt;

use crate::bridge::BridgeMode;
use crate::games::{AUTO, GameProfile, resolve_game_profile};

#[derive(Debug, PartialEq)]
pub enum ConfigError {
    Help(String),
    Message(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Help(text) | Self::Message(text) => formatter.write_str(text),
        }
    }
}

impl From<String> for ConfigError {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}

#[derive(Clone, Debug)]
pub struct BridgeConfig {
    pub game: GameProfile,
    pub listen_host: String,
    pub listen_port: u16,
    pub moza_host: String,
    pub moza_port: u16,
    pub mode: BridgeMode,
    pub fix_tyre_wear_order: bool,
    pub f1_24_car_damage_compat: bool,
    pub input_log: Option<String>,
    pub corner_log: Option<String>,
    pub analysis_report: Option<String>,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub archive_dir: Option<String>,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub analysis_rate_hz: u16,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub raw_retention_days: u32,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub analysis_retention_days: u32,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub acr_finish_distance_m: Option<f32>,
    #[cfg_attr(not(windows), allow(dead_code))]
    pub acr_target_time_s: Option<f32>,
    #[cfg_attr(not(any(target_os = "macos", target_os = "windows")), allow(dead_code))]
    pub headless: bool,
    pub dry_run: bool,
    pub debug: bool,
}

#[derive(Debug, Default)]
struct RawArgs {
    game: Option<String>,
    listen: Option<String>,
    moza_port: Option<String>,
    input_log: Option<String>,
    corner_log: Option<String>,
    analysis_report: Option<String>,
    archive_dir: Option<String>,
    analysis_rate_hz: Option<String>,
    raw_retention_days: Option<String>,
    analysis_retention_days: Option<String>,
    acr_finish_distance_m: Option<String>,
    acr_target_time_s: Option<String>,
    headless: bool,
    debug: bool,
}

pub fn read_config() -> Result<BridgeConfig, ConfigError> {
    parse_config_from(env::args().skip(1))
}

fn parse_config_from<I>(args: I) -> Result<BridgeConfig, ConfigError>
where
    I: IntoIterator<Item = String>,
{
    let raw = parse_raw_args(args)?;
    let game = match raw.game.as_deref() {
        Some(value) => resolve_game_profile(value)?,
        None => AUTO,
    };

    Ok(BridgeConfig {
        game,
        listen_host: "127.0.0.1".to_owned(),
        listen_port: parse_optional_port(
            raw.listen.as_deref(),
            game.default_listen_port.unwrap_or(20777),
            "--listen",
        )?,
        moza_host: "127.0.0.1".to_owned(),
        moza_port: parse_optional_port(
            raw.moza_port.as_deref(),
            game.default_moza_port.unwrap_or(22025),
            "--moza-port",
        )?,
        mode: BridgeMode::Remap,
        fix_tyre_wear_order: false,
        f1_24_car_damage_compat: true,
        input_log: raw.input_log,
        corner_log: raw.corner_log,
        analysis_report: raw.analysis_report,
        archive_dir: raw.archive_dir,
        analysis_rate_hz: parse_range_u16(
            raw.analysis_rate_hz.as_deref(),
            25,
            20,
            50,
            "--analysis-rate-hz",
        )?,
        raw_retention_days: parse_nonzero_u32(
            raw.raw_retention_days.as_deref(),
            7,
            "--raw-retention-days",
        )?,
        analysis_retention_days: parse_nonzero_u32(
            raw.analysis_retention_days.as_deref(),
            90,
            "--analysis-retention-days",
        )?,
        acr_finish_distance_m: parse_optional_positive_f32(
            raw.acr_finish_distance_m.as_deref(),
            "--acr-finish-distance-m",
        )?,
        acr_target_time_s: parse_optional_positive_f32(
            raw.acr_target_time_s.as_deref(),
            "--acr-target-time-s",
        )?,
        headless: raw.headless,
        dry_run: false,
        debug: raw.debug,
    })
}

fn parse_raw_args<I>(args: I) -> Result<RawArgs, ConfigError>
where
    I: IntoIterator<Item = String>,
{
    let mut raw = RawArgs::default();
    let mut iter = args.into_iter();

    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--game" => raw.game = Some(next_value(&mut iter, "--game")?),
            "--listen" => raw.listen = Some(next_value(&mut iter, "--listen")?),
            "--moza-port" => raw.moza_port = Some(next_value(&mut iter, "--moza-port")?),
            "--input-log" => raw.input_log = Some(next_value(&mut iter, "--input-log")?),
            "--corner-log" => raw.corner_log = Some(next_value(&mut iter, "--corner-log")?),
            "--analysis-report" => {
                raw.analysis_report = Some(next_value(&mut iter, "--analysis-report")?)
            }
            "--archive-dir" => raw.archive_dir = Some(next_value(&mut iter, "--archive-dir")?),
            "--analysis-rate-hz" => {
                raw.analysis_rate_hz = Some(next_value(&mut iter, "--analysis-rate-hz")?)
            }
            "--raw-retention-days" => {
                raw.raw_retention_days = Some(next_value(&mut iter, "--raw-retention-days")?)
            }
            "--analysis-retention-days" => {
                raw.analysis_retention_days =
                    Some(next_value(&mut iter, "--analysis-retention-days")?)
            }
            "--acr-finish-distance-m" => {
                raw.acr_finish_distance_m = Some(next_value(&mut iter, "--acr-finish-distance-m")?)
            }
            "--acr-target-time-s" => {
                raw.acr_target_time_s = Some(next_value(&mut iter, "--acr-target-time-s")?)
            }
            "--headless" => raw.headless = true,
            "--debug" => raw.debug = true,
            "--help" | "-h" => return Err(ConfigError::Help(help_text())),
            unknown => {
                return Err(ConfigError::Message(format!(
                    "Unknown option {unknown}\n\n{}",
                    help_text()
                )));
            }
        }
    }

    Ok(raw)
}

fn next_value<I>(iter: &mut I, option: &str) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    iter.next()
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| format!("{option} requires a value"))
}

fn parse_optional_port(value: Option<&str>, fallback: u16, name: &str) -> Result<u16, String> {
    match value {
        Some(raw) => parse_port(raw, name),
        None => Ok(fallback),
    }
}

fn parse_port(raw: &str, name: &str) -> Result<u16, String> {
    let port = raw
        .parse::<u16>()
        .map_err(|_| format!("{name} must be between 1 and 65535"))?;
    if port == 0 {
        return Err(format!("{name} must be between 1 and 65535"));
    }
    Ok(port)
}

fn parse_range_u16(
    value: Option<&str>,
    fallback: u16,
    minimum: u16,
    maximum: u16,
    name: &str,
) -> Result<u16, String> {
    let Some(raw) = value else {
        return Ok(fallback);
    };
    let value = raw
        .parse::<u16>()
        .map_err(|_| format!("{name} must be between {minimum} and {maximum}"))?;
    if !(minimum..=maximum).contains(&value) {
        return Err(format!("{name} must be between {minimum} and {maximum}"));
    }
    Ok(value)
}

fn parse_nonzero_u32(value: Option<&str>, fallback: u32, name: &str) -> Result<u32, String> {
    let Some(raw) = value else {
        return Ok(fallback);
    };
    let value = raw
        .parse::<u32>()
        .map_err(|_| format!("{name} must be a positive whole number"))?;
    if value == 0 {
        return Err(format!("{name} must be a positive whole number"));
    }
    Ok(value)
}

fn parse_optional_positive_f32(value: Option<&str>, name: &str) -> Result<Option<f32>, String> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let value = raw
        .parse::<f32>()
        .map_err(|_| format!("{name} must be a positive number"))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("{name} must be a positive number"));
    }
    Ok(Some(value))
}

fn help_text() -> String {
    [
        "Usage: sim-moza-bridge [options]",
        "",
        "Options:",
        "  --game <auto|f1-25|generic-udp|lmu|lu|ace|acr>",
        "  --listen <port>",
        "  --moza-port <port>",
        "  --input-log <path>",
        "  --corner-log <path>",
        "  --analysis-report <path>",
        "  --archive-dir <path>",
        "  --analysis-rate-hz <20..50>",
        "  --raw-retention-days <days>",
        "  --analysis-retention-days <days>",
        "  --acr-finish-distance-m <meters>",
        "  --acr-target-time-s <seconds>",
        "  --headless",
        "  --debug",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<BridgeConfig, ConfigError> {
        parse_config_from(args.iter().map(|value| (*value).to_owned()))
    }

    #[test]
    fn defaults_to_auto_detect_with_f1_compat() {
        let config = parse(&[]).unwrap();
        assert_eq!(config.game.id, "auto");
        assert_eq!(config.listen_host, "127.0.0.1");
        assert_eq!(config.listen_port, 20777);
        assert_eq!(config.moza_host, "127.0.0.1");
        assert_eq!(config.moza_port, 22025);
        assert_eq!(config.mode, BridgeMode::Remap);
        assert!(!config.fix_tyre_wear_order);
        assert!(config.f1_24_car_damage_compat);
        assert!(!config.headless);
        assert!(!config.dry_run);
        assert!(!config.debug);
        assert_eq!(config.archive_dir, None);
        assert_eq!(config.analysis_rate_hz, 25);
        assert_eq!(config.raw_retention_days, 7);
        assert_eq!(config.analysis_retention_days, 90);
        assert_eq!(config.acr_finish_distance_m, None);
        assert_eq!(config.acr_target_time_s, None);
    }

    #[test]
    fn parses_ports_and_debug() {
        let config = parse(&[
            "--listen",
            "21000",
            "--moza-port",
            "22025",
            "--headless",
            "--debug",
        ])
        .unwrap();

        assert_eq!(config.listen_port, 21000);
        assert_eq!(config.moza_port, 22025);
        assert!(config.headless);
        assert!(config.debug);
    }

    #[test]
    fn parses_logging_and_analysis_outputs() {
        let config = parse(&[
            "--input-log",
            "inputs.csv",
            "--corner-log",
            "corners.csv",
            "--analysis-report",
            "analysis.md",
        ])
        .unwrap();

        assert_eq!(config.input_log.as_deref(), Some("inputs.csv"));
        assert_eq!(config.corner_log.as_deref(), Some("corners.csv"));
        assert_eq!(config.analysis_report.as_deref(), Some("analysis.md"));
    }

    #[test]
    fn parses_acr_capture_policy_and_manual_finish() {
        let config = parse(&[
            "--archive-dir",
            "acr-data",
            "--analysis-rate-hz",
            "40",
            "--raw-retention-days",
            "3",
            "--analysis-retention-days",
            "30",
            "--acr-finish-distance-m",
            "12450.5",
            "--acr-target-time-s",
            "418.25",
        ])
        .unwrap();

        assert_eq!(config.archive_dir.as_deref(), Some("acr-data"));
        assert_eq!(config.analysis_rate_hz, 40);
        assert_eq!(config.raw_retention_days, 3);
        assert_eq!(config.analysis_retention_days, 30);
        assert_eq!(config.acr_finish_distance_m, Some(12_450.5));
        assert_eq!(config.acr_target_time_s, Some(418.25));
    }

    #[test]
    fn rejects_invalid_capture_policy_values() {
        assert!(parse(&["--analysis-rate-hz", "19"]).is_err());
        assert!(parse(&["--analysis-rate-hz", "51"]).is_err());
        assert!(parse(&["--raw-retention-days", "0"]).is_err());
        assert!(parse(&["--analysis-retention-days", "never"]).is_err());
        assert!(parse(&["--acr-finish-distance-m", "NaN"]).is_err());
        assert!(parse(&["--acr-target-time-s", "-1"]).is_err());
    }

    #[test]
    fn parses_game_profiles_that_support_udp_bridge() {
        let f1 = parse(&["--game", "f1-25"]).unwrap();
        assert_eq!(f1.game.id, "f1-25");

        let generic = parse(&["--game", "generic-udp"]).unwrap();
        assert_eq!(generic.game.id, "generic-udp");
    }

    #[test]
    fn parses_shared_memory_game_profiles() {
        let lmu = parse(&["--game", "lmu"]).unwrap();
        assert_eq!(lmu.game.id, "lmu");
        assert_eq!(lmu.listen_port, 20777);
        assert_eq!(lmu.moza_port, 22025);

        let ace = parse(&["--game", "ace"]).unwrap();
        assert_eq!(ace.game.id, "ace");
        assert_eq!(ace.listen_port, 20777);
        assert_eq!(ace.moza_port, 22025);

        let acr = parse(&["--game", "acr"]).unwrap();
        assert_eq!(acr.game.id, "acr");
    }

    #[test]
    fn rejects_removed_options() {
        for option in [
            "--mode",
            "--fix-tyre-wear-order",
            "--f1-24-car-damage-compat",
            "--dry-run",
            "--verbose",
        ] {
            assert!(
                parse(&[option])
                    .unwrap_err()
                    .to_string()
                    .contains("Unknown option")
            );
        }
    }

    #[test]
    fn rejects_options_that_need_values() {
        assert!(
            parse(&["--listen"])
                .unwrap_err()
                .to_string()
                .contains("--listen requires a value")
        );
        assert!(
            parse(&["--moza-port"])
                .unwrap_err()
                .to_string()
                .contains("--moza-port requires a value")
        );
        assert!(
            parse(&["--input-log"])
                .unwrap_err()
                .to_string()
                .contains("--input-log requires a value")
        );
        assert!(
            parse(&["--corner-log"])
                .unwrap_err()
                .to_string()
                .contains("--corner-log requires a value")
        );
        assert!(
            parse(&["--analysis-report"])
                .unwrap_err()
                .to_string()
                .contains("--analysis-report requires a value")
        );
        for option in [
            "--archive-dir",
            "--analysis-rate-hz",
            "--raw-retention-days",
            "--analysis-retention-days",
            "--acr-finish-distance-m",
            "--acr-target-time-s",
        ] {
            assert!(
                parse(&[option])
                    .unwrap_err()
                    .to_string()
                    .contains(&format!("{option} requires a value"))
            );
        }
    }

    #[test]
    fn help_is_distinct_from_startup_errors() {
        assert!(matches!(
            parse(&["--help"]),
            Err(ConfigError::Help(message)) if message.contains("Usage: sim-moza-bridge")
        ));
    }
}
