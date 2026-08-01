use std::env;
use std::fmt;
use std::path::Path;

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
    pub race_engineer: bool,
    pub engineer_voice: bool,
    pub engineer_log: Option<String>,
    pub engineer_state: Option<String>,
    pub engineer_history: Option<String>,
    pub engineer_trigger: Option<String>,
    pub engineer_hook: Option<String>,
    pub engineer_ai_hook: Option<String>,
    pub engineer_ai_task_id: Option<String>,
    pub engineer_radio_hook: Option<String>,
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
    race_engineer: bool,
    engineer_voice: bool,
    engineer_log: Option<String>,
    engineer_state: Option<String>,
    engineer_history: Option<String>,
    engineer_trigger: Option<String>,
    engineer_hook: Option<String>,
    engineer_ai_hook: Option<String>,
    engineer_ai_task_id: Option<String>,
    engineer_radio_hook: Option<String>,
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

    let engineer_ai_hook = raw.engineer_ai_hook;
    let engineer_ai_task_id = raw
        .engineer_ai_task_id
        .map(validate_codex_task_id)
        .transpose()?;
    if engineer_ai_task_id.is_some() && engineer_ai_hook.is_none() {
        return Err(ConfigError::Message(
            "--engineer-ai-task-id requires --engineer-ai-hook".to_owned(),
        ));
    }
    let engineer_state = raw.engineer_state.or_else(|| {
        engineer_ai_hook
            .as_ref()
            .map(|_| "engineer-state.json".to_owned())
    });
    let engineer_history = raw.engineer_history.or_else(|| {
        engineer_state
            .as_deref()
            .map(|path| sibling_output(path, "engineer-history.jsonl"))
    });
    let engineer_trigger = raw.engineer_trigger.or_else(|| {
        engineer_state
            .as_deref()
            .map(|path| sibling_output(path, "engineer-trigger.json"))
            .or_else(|| {
                raw.engineer_hook
                    .as_ref()
                    .map(|_| "engineer-trigger.json".to_owned())
            })
            .or_else(|| {
                engineer_ai_hook
                    .as_ref()
                    .map(|_| "engineer-trigger.json".to_owned())
            })
    });
    let race_engineer = raw.race_engineer
        || raw.engineer_voice
        || raw.engineer_log.is_some()
        || engineer_state.is_some()
        || engineer_history.is_some()
        || engineer_trigger.is_some()
        || raw.engineer_hook.is_some()
        || engineer_ai_hook.is_some()
        || raw.engineer_radio_hook.is_some();

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
        race_engineer,
        engineer_voice: raw.engineer_voice,
        engineer_log: raw.engineer_log,
        engineer_state,
        engineer_history,
        engineer_trigger,
        engineer_hook: raw.engineer_hook,
        engineer_ai_hook,
        engineer_ai_task_id,
        engineer_radio_hook: raw.engineer_radio_hook,
        dry_run: false,
        debug: raw.debug,
    })
}

fn sibling_output(path: &str, file_name: &str) -> String {
    Path::new(path)
        .with_file_name(file_name)
        .to_string_lossy()
        .into_owned()
}

fn validate_codex_task_id(value: String) -> Result<String, String> {
    let bytes = value.as_bytes();
    let valid = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        });
    if valid {
        Ok(value)
    } else {
        Err("--engineer-ai-task-id must be a UUID in 8-4-4-4-12 hexadecimal format".to_owned())
    }
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
            "--race-engineer" => raw.race_engineer = true,
            "--engineer-voice" => raw.engineer_voice = true,
            "--engineer-log" => raw.engineer_log = Some(next_value(&mut iter, "--engineer-log")?),
            "--engineer-state" => {
                raw.engineer_state = Some(next_value(&mut iter, "--engineer-state")?)
            }
            "--engineer-history" => {
                raw.engineer_history = Some(next_value(&mut iter, "--engineer-history")?)
            }
            "--engineer-trigger" => {
                raw.engineer_trigger = Some(next_value(&mut iter, "--engineer-trigger")?)
            }
            "--engineer-hook" => {
                raw.engineer_hook = Some(next_value(&mut iter, "--engineer-hook")?)
            }
            "--engineer-ai-hook" => {
                raw.engineer_ai_hook = Some(next_value(&mut iter, "--engineer-ai-hook")?)
            }
            "--engineer-ai-task-id" => {
                raw.engineer_ai_task_id = Some(next_value(&mut iter, "--engineer-ai-task-id")?)
            }
            "--engineer-radio-hook" => {
                raw.engineer_radio_hook = Some(next_value(&mut iter, "--engineer-radio-hook")?)
            }
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
        "  --race-engineer",
        "  --engineer-voice",
        "  --engineer-log <path>",
        "  --engineer-state <path>",
        "  --engineer-history <path>",
        "  --engineer-trigger <path>",
        "  --engineer-hook <path>",
        "  --engineer-ai-hook <path>",
        "  --engineer-ai-task-id <uuid>",
        "  --engineer-radio-hook <path>",
        "  --debug",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const AI_TASK_ID: &str = "01234567-89ab-cdef-0123-456789abcdef";

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
        assert!(!config.race_engineer);
        assert!(!config.engineer_voice);
        assert_eq!(config.engineer_log, None);
        assert_eq!(config.engineer_state, None);
        assert_eq!(config.engineer_history, None);
        assert_eq!(config.engineer_trigger, None);
        assert_eq!(config.engineer_hook, None);
        assert_eq!(config.engineer_ai_hook, None);
        assert_eq!(config.engineer_ai_task_id, None);
        assert_eq!(config.engineer_radio_hook, None);
        assert!(!config.dry_run);
        assert!(!config.debug);
    }

    #[test]
    fn parses_ports_and_debug() {
        let config = parse(&["--listen", "21000", "--moza-port", "22025", "--debug"]).unwrap();

        assert_eq!(config.listen_port, 21000);
        assert_eq!(config.moza_port, 22025);
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
    fn parses_race_engineer_outputs() {
        let config = parse(&[
            "--engineer-voice",
            "--engineer-log",
            "engineer.jsonl",
            "--engineer-state",
            "engineer-state.json",
            "--engineer-history",
            "engineer-history.jsonl",
            "--engineer-trigger",
            "engineer-trigger.json",
            "--engineer-hook",
            "engineer-hook.ps1",
            "--engineer-ai-hook",
            "ai-engineer-hook.ps1",
            "--engineer-ai-task-id",
            AI_TASK_ID,
            "--engineer-radio-hook",
            "engineer-radio-hook.ps1",
        ])
        .unwrap();

        assert!(config.race_engineer);
        assert!(config.engineer_voice);
        assert_eq!(config.engineer_log.as_deref(), Some("engineer.jsonl"));
        assert_eq!(
            config.engineer_state.as_deref(),
            Some("engineer-state.json")
        );
        assert_eq!(
            config.engineer_history.as_deref(),
            Some("engineer-history.jsonl")
        );
        assert_eq!(
            config.engineer_trigger.as_deref(),
            Some("engineer-trigger.json")
        );
        assert_eq!(config.engineer_hook.as_deref(), Some("engineer-hook.ps1"));
        assert_eq!(
            config.engineer_ai_hook.as_deref(),
            Some("ai-engineer-hook.ps1")
        );
        assert_eq!(config.engineer_ai_task_id.as_deref(), Some(AI_TASK_ID));
        assert_eq!(
            config.engineer_radio_hook.as_deref(),
            Some("engineer-radio-hook.ps1")
        );

        let console_only = parse(&["--race-engineer"]).unwrap();
        assert!(console_only.race_engineer);
        assert!(!console_only.engineer_voice);

        let hook_only = parse(&["--engineer-hook", "hook.exe"]).unwrap();
        assert!(hook_only.race_engineer);
        assert_eq!(
            hook_only.engineer_trigger.as_deref(),
            Some("engineer-trigger.json")
        );

        let radio_hook_only = parse(&["--engineer-radio-hook", "radio-hook.exe"]).unwrap();
        assert!(radio_hook_only.race_engineer);
        assert_eq!(
            radio_hook_only.engineer_radio_hook.as_deref(),
            Some("radio-hook.exe")
        );

        let ai_hook_only = parse(&["--engineer-ai-hook", "ai-hook.ps1"]).unwrap();
        assert!(ai_hook_only.race_engineer);
        assert_eq!(
            ai_hook_only.engineer_ai_hook.as_deref(),
            Some("ai-hook.ps1")
        );
        assert_eq!(
            ai_hook_only.engineer_state.as_deref(),
            Some("engineer-state.json")
        );
        assert_eq!(
            ai_hook_only.engineer_trigger.as_deref(),
            Some("engineer-trigger.json")
        );
        assert_eq!(ai_hook_only.engineer_ai_task_id, None);

        let state_only = parse(&["--engineer-state", "live/state.json"]).unwrap();
        let expected_history = Path::new("live").join("engineer-history.jsonl");
        let expected_trigger = Path::new("live").join("engineer-trigger.json");
        assert_eq!(
            state_only.engineer_history.as_deref().map(Path::new),
            Some(expected_history.as_path())
        );
        assert_eq!(
            state_only.engineer_trigger.as_deref().map(Path::new),
            Some(expected_trigger.as_path())
        );
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
        assert!(
            parse(&["--engineer-log"])
                .unwrap_err()
                .to_string()
                .contains("--engineer-log requires a value")
        );
        assert!(
            parse(&["--engineer-state"])
                .unwrap_err()
                .to_string()
                .contains("--engineer-state requires a value")
        );
        assert!(
            parse(&["--engineer-history"])
                .unwrap_err()
                .to_string()
                .contains("--engineer-history requires a value")
        );
        assert!(
            parse(&["--engineer-trigger"])
                .unwrap_err()
                .to_string()
                .contains("--engineer-trigger requires a value")
        );
        assert!(
            parse(&["--engineer-hook"])
                .unwrap_err()
                .to_string()
                .contains("--engineer-hook requires a value")
        );
        assert!(
            parse(&["--engineer-ai-hook"])
                .unwrap_err()
                .to_string()
                .contains("--engineer-ai-hook requires a value")
        );
        assert!(
            parse(&["--engineer-ai-task-id"])
                .unwrap_err()
                .to_string()
                .contains("--engineer-ai-task-id requires a value")
        );
        assert!(
            parse(&["--engineer-radio-hook"])
                .unwrap_err()
                .to_string()
                .contains("--engineer-radio-hook requires a value")
        );
    }

    #[test]
    fn validates_per_launch_ai_task_id() {
        assert!(
            parse(&["--engineer-ai-task-id", AI_TASK_ID])
                .unwrap_err()
                .to_string()
                .contains("requires --engineer-ai-hook")
        );
        assert!(
            parse(&[
                "--engineer-ai-hook",
                "ai-hook.ps1",
                "--engineer-ai-task-id",
                "not-a-task-id",
            ])
            .unwrap_err()
            .to_string()
            .contains("must be a UUID")
        );
    }

    #[test]
    fn help_is_distinct_from_startup_errors() {
        assert!(matches!(
            parse(&["--help"]),
            Err(ConfigError::Help(message)) if message.contains("Usage: sim-moza-bridge")
        ));
    }
}
