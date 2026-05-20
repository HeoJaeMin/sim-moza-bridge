pub const F1_25_PACKET_FORMAT: u16 = 2025;
pub const F1_24_PACKET_FORMAT: u16 = 2024;
pub const PACKET_HEADER_SIZE: usize = 29;
pub const MAX_CARS: usize = 22;

pub mod packet_id {
    pub const MOTION: u8 = 0;
    pub const SESSION: u8 = 1;
    pub const LAP_DATA: u8 = 2;
    pub const EVENT: u8 = 3;
    pub const PARTICIPANTS: u8 = 4;
    pub const CAR_SETUPS: u8 = 5;
    pub const CAR_TELEMETRY: u8 = 6;
    pub const CAR_STATUS: u8 = 7;
    pub const FINAL_CLASSIFICATION: u8 = 8;
    pub const LOBBY_INFO: u8 = 9;
    pub const CAR_DAMAGE: u8 = 10;
    pub const SESSION_HISTORY: u8 = 11;
    pub const TYRE_SETS: u8 = 12;
    pub const MOTION_EX: u8 = 13;
    pub const TIME_TRIAL: u8 = 14;
    pub const LAP_POSITIONS: u8 = 15;
}

pub fn packet_name(id: u8) -> String {
    let name = match id {
        packet_id::MOTION => "Motion",
        packet_id::SESSION => "Session",
        packet_id::LAP_DATA => "LapData",
        packet_id::EVENT => "Event",
        packet_id::PARTICIPANTS => "Participants",
        packet_id::CAR_SETUPS => "CarSetups",
        packet_id::CAR_TELEMETRY => "CarTelemetry",
        packet_id::CAR_STATUS => "CarStatus",
        packet_id::FINAL_CLASSIFICATION => "FinalClassification",
        packet_id::LOBBY_INFO => "LobbyInfo",
        packet_id::CAR_DAMAGE => "CarDamage",
        packet_id::SESSION_HISTORY => "SessionHistory",
        packet_id::TYRE_SETS => "TyreSets",
        packet_id::MOTION_EX => "MotionEx",
        packet_id::TIME_TRIAL => "TimeTrial",
        packet_id::LAP_POSITIONS => "LapPositions",
        _ => return id.to_string(),
    };
    name.to_owned()
}
