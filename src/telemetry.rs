#[derive(Clone, Debug, PartialEq)]
pub struct InputSample {
    pub session_time: f32,
    pub frame_identifier: u32,
    pub player_car_index: u8,
    pub throttle: f32,
    pub brake: f32,
    pub speed_kmh: u16,
    pub gear: i8,
    pub rpm: u16,
}

impl InputSample {
    pub fn csv_header() -> &'static str {
        "session_time,frame_identifier,player_car_index,throttle,brake,speed_kmh,gear,rpm\n"
    }

    pub fn to_csv_row(&self) -> String {
        format!(
            "{:.3},{},{},{:.5},{:.5},{},{},{}\n",
            self.session_time,
            self.frame_identifier,
            self.player_car_index,
            self.throttle,
            self.brake,
            self.speed_kmh,
            self.gear,
            self.rpm
        )
    }

    pub fn to_json(&self) -> String {
        format!(
            concat!(
                "{{",
                "\"sessionTime\":{:.3},",
                "\"frameIdentifier\":{},",
                "\"playerCarIndex\":{},",
                "\"throttle\":{:.5},",
                "\"brake\":{:.5},",
                "\"speedKmh\":{},",
                "\"gear\":{},",
                "\"rpm\":{}",
                "}}"
            ),
            self.session_time,
            self.frame_identifier,
            self.player_car_index,
            self.throttle,
            self.brake,
            self.speed_kmh,
            self.gear,
            self.rpm
        )
    }
}
