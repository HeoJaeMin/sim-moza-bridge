use std::env;
use std::fmt;

use crate::bridge::BridgeMode;
use crate::games::{GameProfile, assert_udp_bridge_supported, resolve_game_profile};

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
    pub input_log: Option<String>,
    pub corner_log: Option<String>,
    pub analysis_report: Option<String>,
    pub hud_host: String,
    pub hud_http_port: Option<u16>,
    pub dry_run: bool,
    pub verbose: bool,
}

#[derive(Debug)]
struct RawArgs {
    game: String,
    listen_host: String,
    listen: Option<String>,
    moza_host: String,
    moza_port: Option<String>,
    mode: String,
    fix_tyre_wear_order: bool,
    input_log: Option<String>,
    corner_log: Option<String>,
    analysis_report: Option<String>,
    hud_host: String,
    hud_http_port: Option<String>,
    dry_run: bool,
    verbose: bool,
}

impl Default for RawArgs {
    fn default() -> Self {
        Self {
            game: "auto".to_owned(),
            listen_host: "127.0.0.1".to_owned(),
            listen: None,
            moza_host: "127.0.0.1".to_owned(),
            moza_port: None,
            mode: "passthrough".to_owned(),
            fix_tyre_wear_order: false,
            input_log: None,
            corner_log: None,
            analysis_report: None,
            hud_host: "127.0.0.1".to_owned(),
            hud_http_port: None,
            dry_run: false,
            verbose: false,
        }
    }
}

pub fn read_config() -> Result<BridgeConfig, ConfigError> {
    parse_config_from(env::args().skip(1))
}

fn parse_config_from<I>(args: I) -> Result<BridgeConfig, ConfigError>
where
    I: IntoIterator<Item = String>,
{
    let raw = parse_raw_args(args)?;
    let game = resolve_game_profile(&raw.game)?;
    assert_udp_bridge_supported(game)?;

    if raw.fix_tyre_wear_order && !game.supports_tyre_wear_order_fix {
        return Err(ConfigError::Message(
            "--fix-tyre-wear-order is only supported for F1-compatible or auto-detected profiles."
                .to_owned(),
        ));
    }

    Ok(BridgeConfig {
        game,
        listen_host: raw.listen_host,
        listen_port: parse_optional_port(
            raw.listen.as_deref(),
            game.default_listen_port.unwrap_or(20777),
            "--listen",
        )?,
        moza_host: raw.moza_host,
        moza_port: parse_optional_port(
            raw.moza_port.as_deref(),
            game.default_moza_port.unwrap_or(22025),
            "--moza-port",
        )?,
        mode: parse_mode(&raw.mode)?,
        fix_tyre_wear_order: raw.fix_tyre_wear_order,
        input_log: raw.input_log,
        corner_log: raw.corner_log,
        analysis_report: raw.analysis_report,
        hud_host: raw.hud_host,
        hud_http_port: parse_optional_present_port(raw.hud_http_port.as_deref(), "--hud-http")?,
        dry_run: raw.dry_run,
        verbose: raw.verbose,
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
            "--game" => raw.game = next_value(&mut iter, "--game")?,
            "--listen-host" => raw.listen_host = next_value(&mut iter, "--listen-host")?,
            "--listen" => raw.listen = Some(next_value(&mut iter, "--listen")?),
            "--moza-host" => raw.moza_host = next_value(&mut iter, "--moza-host")?,
            "--moza-port" => raw.moza_port = Some(next_value(&mut iter, "--moza-port")?),
            "--mode" => raw.mode = next_value(&mut iter, "--mode")?,
            "--fix-tyre-wear-order" => raw.fix_tyre_wear_order = true,
            "--input-log" => raw.input_log = Some(next_value(&mut iter, "--input-log")?),
            "--corner-log" => raw.corner_log = Some(next_value(&mut iter, "--corner-log")?),
            "--analysis-report" => {
                raw.analysis_report = Some(next_value(&mut iter, "--analysis-report")?)
            }
            "--hud-host" => raw.hud_host = next_value(&mut iter, "--hud-host")?,
            "--hud-http" => raw.hud_http_port = Some(next_value(&mut iter, "--hud-http")?),
            "--dry-run" => raw.dry_run = true,
            "--verbose" => raw.verbose = true,
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

fn parse_optional_present_port(value: Option<&str>, name: &str) -> Result<Option<u16>, String> {
    match value {
        Some(raw) => parse_port(raw, name).map(Some),
        None => Ok(None),
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

fn parse_mode(value: &str) -> Result<BridgeMode, String> {
    match value {
        "passthrough" => Ok(BridgeMode::Passthrough),
        "remap" => Ok(BridgeMode::Remap),
        _ => Err("--mode must be passthrough or remap".to_owned()),
    }
}

fn help_text() -> String {
    [
        "Usage: sim-moza-bridge [options]",
        "",
        "Options:",
        "  --game <auto|f1-25|generic-udp|ace|lmu>",
        "  --listen <port>",
        "  --listen-host <host>",
        "  --moza-host <host>",
        "  --moza-port <port>",
        "  --mode <passthrough|remap>",
        "  --fix-tyre-wear-order",
        "  --input-log <csv-path>",
        "  --corner-log <csv-path>",
        "  --analysis-report <md-path>",
        "  --hud-http <port>",
        "  --hud-host <host>",
        "  --dry-run",
        "  --verbose",
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
    fn defaults_to_auto() {
        let config = parse(&[]).unwrap();
        assert_eq!(config.game.id, "auto");
        assert_eq!(config.listen_host, "127.0.0.1");
        assert_eq!(config.listen_port, 20777);
        assert_eq!(config.moza_port, 22025);
        assert_eq!(config.mode, BridgeMode::Passthrough);
        assert_eq!(config.hud_http_port, None);
    }

    #[test]
    fn parses_f1_remap_options() {
        let config = parse(&[
            "--game",
            "f1-25",
            "--listen",
            "21000",
            "--moza-port",
            "22025",
            "--mode",
            "remap",
            "--fix-tyre-wear-order",
            "--input-log",
            "inputs.csv",
            "--corner-log",
            "corners.csv",
            "--analysis-report",
            "analysis.md",
            "--hud-http",
            "8080",
            "--dry-run",
            "--verbose",
        ])
        .unwrap();

        assert_eq!(config.game.id, "f1-25");
        assert_eq!(config.listen_port, 21000);
        assert_eq!(config.mode, BridgeMode::Remap);
        assert!(config.fix_tyre_wear_order);
        assert_eq!(config.input_log.as_deref(), Some("inputs.csv"));
        assert_eq!(config.corner_log.as_deref(), Some("corners.csv"));
        assert_eq!(config.analysis_report.as_deref(), Some("analysis.md"));
        assert_eq!(config.hud_http_port, Some(8080));
        assert!(config.dry_run);
        assert!(config.verbose);
    }

    #[test]
    fn rejects_non_udp_profiles() {
        assert!(
            parse(&["--game", "ace"])
                .unwrap_err()
                .to_string()
                .contains("not a UDP bridge profile")
        );
        assert!(
            parse(&["--game", "lmu"])
                .unwrap_err()
                .to_string()
                .contains("not a UDP bridge profile")
        );
    }

    #[test]
    fn rejects_fix_for_generic_udp() {
        assert!(
            parse(&["--game", "generic-udp", "--fix-tyre-wear-order"])
                .unwrap_err()
                .to_string()
                .contains("F1-compatible")
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
