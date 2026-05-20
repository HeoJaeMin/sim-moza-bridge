#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputKind {
    Udp,
    SharedMemory,
    PluginSharedMemory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolKind {
    Auto,
    F1_25,
    OpaqueUdp,
    AssettoCorsaEvo,
    AssettoCorsaRally,
    LeMansUltimate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GameProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub input_kind: InputKind,
    pub protocol: ProtocolKind,
    pub default_listen_port: Option<u16>,
    pub default_moza_port: Option<u16>,
    pub supports_udp_bridge: bool,
    pub supports_tyre_wear_order_fix: bool,
    pub notes: &'static [&'static str],
}

pub const AUTO: GameProfile = GameProfile {
    id: "auto",
    name: "Auto detect",
    aliases: &["auto", "detect", "auto-detect"],
    input_kind: InputKind::Udp,
    protocol: ProtocolKind::Auto,
    default_listen_port: Some(20777),
    default_moza_port: Some(22025),
    supports_udp_bridge: true,
    supports_tyre_wear_order_fix: true,
    notes: &[
        "Detects supported games from incoming UDP packets.",
        "Currently recognizes F1 25 packets and keeps unknown packets as raw UDP passthrough.",
    ],
};

pub const F1_25: GameProfile = GameProfile {
    id: "f1-25",
    name: "F1 25",
    aliases: &["f1", "f125", "f1-2025", "f1-25"],
    input_kind: InputKind::Udp,
    protocol: ProtocolKind::F1_25,
    default_listen_port: Some(20777),
    default_moza_port: Some(22025),
    supports_udp_bridge: true,
    supports_tyre_wear_order_fix: true,
    notes: &[
        "F1 25 emits binary UDP telemetry.",
        "MOZA Pit House expects the F1 25 telemetry port to be 22025.",
    ],
};

pub const GENERIC_UDP: GameProfile = GameProfile {
    id: "generic-udp",
    name: "Generic UDP passthrough",
    aliases: &["generic", "udp", "raw-udp"],
    input_kind: InputKind::Udp,
    protocol: ProtocolKind::OpaqueUdp,
    default_listen_port: Some(20777),
    default_moza_port: Some(22025),
    supports_udp_bridge: true,
    supports_tyre_wear_order_fix: false,
    notes: &[
        "Use this when another tool already emits telemetry in a format the target app understands.",
        "Packets are forwarded unchanged and no game-specific parser is applied.",
    ],
};

pub const ACE: GameProfile = GameProfile {
    id: "ace",
    name: "Assetto Corsa EVO",
    aliases: &["ace", "ac-evo", "ac-evo-early-access", "assetto-corsa-evo"],
    input_kind: InputKind::SharedMemory,
    protocol: ProtocolKind::AssettoCorsaEvo,
    default_listen_port: None,
    default_moza_port: None,
    supports_udp_bridge: false,
    supports_tyre_wear_order_fix: false,
    notes: &[
        "ACE exposes telemetry through local shared-memory style integrations, not a simple UDP output.",
        "The current adapter reads the ACE physics shared-memory mapping for HUD telemetry.",
    ],
};

pub const ACR: GameProfile = GameProfile {
    id: "acr",
    name: "Assetto Corsa Rally",
    aliases: &["acr", "ac-rally", "assetto-corsa-rally"],
    input_kind: InputKind::SharedMemory,
    protocol: ProtocolKind::AssettoCorsaRally,
    default_listen_port: None,
    default_moza_port: None,
    supports_udp_bridge: false,
    supports_tyre_wear_order_fix: false,
    notes: &[
        "MOZA lists telemetry support for ACR, but the digital-dash key matrix does not expose an ACR-specific column yet.",
        "Public overlay tooling points to a native/helper memory reader path rather than F1-style UDP packets.",
    ],
};

pub const LMU: GameProfile = GameProfile {
    id: "lmu",
    name: "Le Mans Ultimate",
    aliases: &["lmu", "lu", "le-mans-ultimate"],
    input_kind: InputKind::PluginSharedMemory,
    protocol: ProtocolKind::LeMansUltimate,
    default_listen_port: None,
    default_moza_port: None,
    supports_udp_bridge: false,
    supports_tyre_wear_order_fix: false,
    notes: &[
        "MOZA lists native telemetry support for LMU and the digital-dash key matrix includes a Le mans ultimate column.",
        "The current adapter reads LMU_Data shared memory for HUD telemetry.",
    ],
};

pub const GAME_PROFILES: &[GameProfile] = &[AUTO, F1_25, GENERIC_UDP, ACE, ACR, LMU];

pub fn list_game_profile_ids() -> String {
    GAME_PROFILES
        .iter()
        .map(|profile| profile.id)
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn resolve_game_profile(value: &str) -> Result<GameProfile, String> {
    let normalized = value.trim().to_ascii_lowercase();
    GAME_PROFILES
        .iter()
        .copied()
        .find(|profile| profile.id == normalized || profile.aliases.contains(&normalized.as_str()))
        .ok_or_else(|| {
            format!(
                "Unsupported game \"{value}\". Supported game profiles: {}",
                list_game_profile_ids()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_ids_and_aliases() {
        assert_eq!(resolve_game_profile("auto").unwrap().id, "auto");
        assert_eq!(resolve_game_profile("F125").unwrap().id, "f1-25");
        assert_eq!(resolve_game_profile("LU").unwrap().id, "lmu");
        assert_eq!(resolve_game_profile("ace").unwrap().id, "ace");
        assert_eq!(resolve_game_profile("ac-rally").unwrap().id, "acr");
    }

    #[test]
    fn rejects_unsupported_profile() {
        let message = resolve_game_profile("iracing").unwrap_err();
        assert!(message.contains("Supported game profiles"));
    }
}
