use std::collections::BTreeMap;

use crate::detect::detect_game_profile_from_packet;
use crate::f1::car_damage::{
    parse_player_damage_sample, rewrite_all_tyre_wear_to_moza_named_order,
    to_f1_24_car_damage_compat_packet,
};
use crate::f1::car_status::parse_player_status_sample;
use crate::f1::car_telemetry::parse_player_input_sample;
use crate::f1::constants::{F1_25_PACKET_FORMAT, packet_id, packet_name};
use crate::f1::header::{PacketHeader, parse_packet_header};
use crate::f1::lap_data::parse_player_lap_sample;
use crate::f1::session::parse_session_sample;
use crate::games::{GameProfile, ProtocolKind};
use crate::telemetry::TelemetryUpdate;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeMode {
    Passthrough,
    Remap,
}

#[derive(Debug, Default)]
pub struct BridgeStats {
    pub received: u64,
    pub forwarded: u64,
    pub patched: u64,
    pub ignored: u64,
    pub malformed: u64,
    pub by_packet_id: BTreeMap<u8, u64>,
}

#[derive(Debug)]
pub struct ProcessedPacket {
    pub packet: Vec<u8>,
    pub patched: bool,
    pub detected_game: Option<GameProfile>,
    pub telemetry_update: TelemetryUpdate,
}

pub struct TelemetryBridge {
    game: GameProfile,
    active_game: GameProfile,
    mode: BridgeMode,
    fix_tyre_wear_order: bool,
    f1_24_car_damage_compat: bool,
    pub stats: BridgeStats,
}

impl TelemetryBridge {
    pub fn new(
        game: GameProfile,
        mode: BridgeMode,
        fix_tyre_wear_order: bool,
        f1_24_car_damage_compat: bool,
    ) -> Self {
        Self {
            game,
            active_game: game,
            mode,
            fix_tyre_wear_order,
            f1_24_car_damage_compat,
            stats: BridgeStats::default(),
        }
    }

    pub fn process(&mut self, packet: &[u8]) -> Option<ProcessedPacket> {
        self.stats.received += 1;

        let detected_game = self.detect_active_game(packet);
        let protocol = detected_game
            .map(|game| game.protocol)
            .unwrap_or(self.active_game.protocol);

        if matches!(protocol, ProtocolKind::Auto | ProtocolKind::OpaqueUdp) {
            return Some(ProcessedPacket {
                packet: packet.to_vec(),
                patched: false,
                detected_game,
                telemetry_update: TelemetryUpdate::default(),
            });
        }

        let header = self.parse_known_protocol_header(packet, protocol)?;
        let telemetry_update = if is_supported_f1_25_header(&header) {
            parse_telemetry_update(packet, header.packet_id)
        } else {
            TelemetryUpdate::default()
        };
        if self.mode == BridgeMode::Passthrough {
            return Some(ProcessedPacket {
                packet: packet.to_vec(),
                patched: false,
                detected_game,
                telemetry_update,
            });
        }

        if !is_supported_f1_25_header(&header) {
            self.stats.ignored += 1;
            return Some(ProcessedPacket {
                packet: packet.to_vec(),
                patched: false,
                detected_game,
                telemetry_update,
            });
        }

        if header.packet_id == packet_id::CAR_DAMAGE
            && (self.fix_tyre_wear_order || self.f1_24_car_damage_compat)
        {
            let mut patched_packet = packet.to_vec();

            if self.fix_tyre_wear_order
                && !rewrite_all_tyre_wear_to_moza_named_order(&mut patched_packet)
            {
                self.stats.malformed += 1;
                return Some(ProcessedPacket {
                    packet: packet.to_vec(),
                    patched: false,
                    detected_game,
                    telemetry_update,
                });
            }

            if self.f1_24_car_damage_compat {
                let Some(compat_packet) = to_f1_24_car_damage_compat_packet(&patched_packet) else {
                    self.stats.malformed += 1;
                    return Some(ProcessedPacket {
                        packet: packet.to_vec(),
                        patched: false,
                        detected_game,
                        telemetry_update,
                    });
                };
                patched_packet = compat_packet;
            }

            self.stats.patched += 1;
            return Some(ProcessedPacket {
                packet: patched_packet,
                patched: true,
                detected_game,
                telemetry_update,
            });
        }

        Some(ProcessedPacket {
            packet: packet.to_vec(),
            patched: false,
            detected_game,
            telemetry_update,
        })
    }

    pub fn mark_forwarded(&mut self) {
        self.stats.forwarded += 1;
    }

    pub fn packet_summary(&self) -> String {
        let entries = self
            .stats
            .by_packet_id
            .iter()
            .map(|(id, count)| format!("{}:{count}", packet_name(*id)))
            .collect::<Vec<_>>();

        if entries.is_empty() {
            "none".to_owned()
        } else {
            entries.join(" ")
        }
    }

    fn detect_active_game(&mut self, packet: &[u8]) -> Option<GameProfile> {
        if self.active_game.protocol != ProtocolKind::Auto {
            return None;
        }

        let detected_game = detect_game_profile_from_packet(packet)?;
        self.active_game = detected_game;
        Some(detected_game)
    }

    fn parse_known_protocol_header(
        &mut self,
        packet: &[u8],
        protocol: ProtocolKind,
    ) -> Option<PacketHeader> {
        if protocol != ProtocolKind::F1_25 {
            self.stats.ignored += 1;
            return None;
        }

        let header = parse_packet_header(packet);
        if header.is_none() {
            self.stats.malformed += 1;
            return None;
        }

        let header = header?;
        *self.stats.by_packet_id.entry(header.packet_id).or_insert(0) += 1;
        Some(header)
    }

    #[allow(dead_code)]
    pub fn configured_game(&self) -> GameProfile {
        self.game
    }
}

fn is_supported_f1_25_header(header: &PacketHeader) -> bool {
    header.packet_format == F1_25_PACKET_FORMAT && header.game_year == 25
}

fn parse_telemetry_update(packet: &[u8], packet_id: u8) -> TelemetryUpdate {
    let mut update = TelemetryUpdate::default();

    match packet_id {
        packet_id::SESSION => update.session = parse_session_sample(packet).ok(),
        packet_id::LAP_DATA => update.lap = parse_player_lap_sample(packet).ok(),
        packet_id::CAR_TELEMETRY => update.input = parse_player_input_sample(packet).ok(),
        packet_id::CAR_STATUS => update.status = parse_player_status_sample(packet).ok(),
        packet_id::CAR_DAMAGE => update.damage = parse_player_damage_sample(packet).ok(),
        _ => {}
    }

    update
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::f1::car_damage::{
        CAR_DAMAGE_PACKET_SIZE, F1_24_CAR_DAMAGE_PACKET_SIZE, car_damage_offset,
    };
    use crate::f1::car_telemetry::{CAR_TELEMETRY_PACKET_SIZE, car_telemetry_offset};
    use crate::games::resolve_game_profile;

    fn make_f1_car_damage_packet() -> Vec<u8> {
        let mut packet = vec![0_u8; CAR_DAMAGE_PACKET_SIZE];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::CAR_DAMAGE;
        let base = car_damage_offset(0).unwrap();
        packet[base..base + 4].copy_from_slice(&10.0_f32.to_le_bytes());
        packet[base + 4..base + 8].copy_from_slice(&20.0_f32.to_le_bytes());
        packet[base + 8..base + 12].copy_from_slice(&30.0_f32.to_le_bytes());
        packet[base + 12..base + 16].copy_from_slice(&40.0_f32.to_le_bytes());
        packet
    }

    fn read_raw_wear(packet: &[u8]) -> [f32; 4] {
        let base = car_damage_offset(0).unwrap();
        [
            f32::from_le_bytes(packet[base..base + 4].try_into().unwrap()),
            f32::from_le_bytes(packet[base + 4..base + 8].try_into().unwrap()),
            f32::from_le_bytes(packet[base + 8..base + 12].try_into().unwrap()),
            f32::from_le_bytes(packet[base + 12..base + 16].try_into().unwrap()),
        ]
    }

    fn make_f1_car_telemetry_packet() -> Vec<u8> {
        let mut packet = vec![0_u8; CAR_TELEMETRY_PACKET_SIZE];
        packet[0..2].copy_from_slice(&F1_25_PACKET_FORMAT.to_le_bytes());
        packet[2] = 25;
        packet[6] = packet_id::CAR_TELEMETRY;
        packet[15..19].copy_from_slice(&22.0_f32.to_le_bytes());
        packet[19..23].copy_from_slice(&88_u32.to_le_bytes());
        packet[27] = 0;

        let base = car_telemetry_offset(0).unwrap();
        packet[base..base + 2].copy_from_slice(&123_u16.to_le_bytes());
        packet[base + 2..base + 6].copy_from_slice(&0.5_f32.to_le_bytes());
        packet[base + 10..base + 14].copy_from_slice(&0.125_f32.to_le_bytes());
        packet[base + 15] = 4_u8;
        packet[base + 16..base + 18].copy_from_slice(&9000_u16.to_le_bytes());
        packet
    }

    fn make_unsupported_car_telemetry_packet() -> Vec<u8> {
        let mut packet = make_f1_car_telemetry_packet();
        packet[0..2].copy_from_slice(&2024_u16.to_le_bytes());
        packet[2] = 24;
        packet
    }

    #[test]
    fn generic_udp_forwards_opaque_packets_without_parsing() {
        let mut bridge = TelemetryBridge::new(
            resolve_game_profile("generic-udp").unwrap(),
            BridgeMode::Passthrough,
            false,
            false,
        );
        let packet = vec![1_u8, 2, 3];

        let result = bridge.process(&packet).unwrap();

        assert_eq!(result.packet, packet);
        assert!(!result.patched);
        assert_eq!(bridge.stats.malformed, 0);
        assert_eq!(bridge.stats.received, 1);
    }

    #[test]
    fn f1_profile_rejects_malformed_packets() {
        let mut bridge = TelemetryBridge::new(
            resolve_game_profile("f1-25").unwrap(),
            BridgeMode::Passthrough,
            false,
            false,
        );

        assert!(bridge.process(&[1, 2, 3]).is_none());
        assert_eq!(bridge.stats.malformed, 1);
    }

    #[test]
    fn auto_profile_keeps_unknown_packets_open_for_later_detection() {
        let mut bridge = TelemetryBridge::new(
            resolve_game_profile("auto").unwrap(),
            BridgeMode::Remap,
            true,
            false,
        );

        let unknown = vec![1_u8, 2, 3];
        assert_eq!(bridge.process(&unknown).unwrap().packet, unknown);
        assert_eq!(bridge.stats.malformed, 0);

        let result = bridge.process(&make_f1_car_damage_packet()).unwrap();
        assert_eq!(result.detected_game.unwrap().id, "f1-25");
        assert!(result.patched);
        assert_eq!(read_raw_wear(&result.packet), [30.0, 40.0, 10.0, 20.0]);
    }

    #[test]
    fn auto_profile_applies_f1_compat_after_detection() {
        let mut bridge = TelemetryBridge::new(
            resolve_game_profile("auto").unwrap(),
            BridgeMode::Remap,
            false,
            true,
        );

        let result = bridge.process(&make_f1_car_damage_packet()).unwrap();
        let header = parse_packet_header(&result.packet).unwrap();

        assert_eq!(result.detected_game.unwrap().id, "f1-25");
        assert!(result.patched);
        assert_eq!(header.packet_format, 2024);
        assert_eq!(header.game_year, 24);
        assert_eq!(result.packet.len(), F1_24_CAR_DAMAGE_PACKET_SIZE);
    }

    #[test]
    fn f1_car_telemetry_packets_emit_input_samples() {
        let mut bridge = TelemetryBridge::new(
            resolve_game_profile("f1-25").unwrap(),
            BridgeMode::Passthrough,
            false,
            false,
        );

        let result = bridge.process(&make_f1_car_telemetry_packet()).unwrap();
        let sample = result.telemetry_update.input.unwrap();

        assert_eq!(sample.frame_identifier, 88);
        assert_eq!(sample.throttle, 0.5);
        assert_eq!(sample.brake, 0.125);
        assert_eq!(sample.speed_kmh, 123);
        assert_eq!(sample.gear, 4);
        assert_eq!(sample.rpm, 9000);
    }

    #[test]
    fn f1_profile_does_not_parse_unsupported_formats_for_analysis() {
        let mut bridge = TelemetryBridge::new(
            resolve_game_profile("f1-25").unwrap(),
            BridgeMode::Passthrough,
            false,
            false,
        );

        let result = bridge
            .process(&make_unsupported_car_telemetry_packet())
            .unwrap();

        assert!(result.telemetry_update.is_empty());
    }

    #[test]
    fn f1_25_car_damage_can_emit_f1_24_compat_packet() {
        let mut bridge = TelemetryBridge::new(
            resolve_game_profile("f1-25").unwrap(),
            BridgeMode::Remap,
            false,
            true,
        );

        let result = bridge.process(&make_f1_car_damage_packet()).unwrap();
        let header = parse_packet_header(&result.packet).unwrap();

        assert!(result.patched);
        assert_eq!(header.packet_format, 2024);
        assert_eq!(header.game_year, 24);
        assert_eq!(result.packet.len(), F1_24_CAR_DAMAGE_PACKET_SIZE);
        assert_eq!(read_raw_wear(&result.packet), [10.0, 20.0, 30.0, 40.0]);
    }

    #[test]
    fn f1_25_car_damage_compat_respects_tyre_wear_order_fix() {
        let mut bridge = TelemetryBridge::new(
            resolve_game_profile("f1-25").unwrap(),
            BridgeMode::Remap,
            true,
            true,
        );

        let result = bridge.process(&make_f1_car_damage_packet()).unwrap();
        let header = parse_packet_header(&result.packet).unwrap();

        assert!(result.patched);
        assert_eq!(header.packet_format, 2024);
        assert_eq!(header.game_year, 24);
        assert_eq!(result.packet.len(), F1_24_CAR_DAMAGE_PACKET_SIZE);
        assert_eq!(read_raw_wear(&result.packet), [30.0, 40.0, 10.0, 20.0]);
    }
}
