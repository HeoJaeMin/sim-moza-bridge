use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct WheelValuesF32 {
    pub rl: f32,
    pub rr: f32,
    pub fl: f32,
    pub fr: f32,
}

impl WheelValuesF32 {
    pub fn front_avg(&self) -> f32 {
        (self.fl + self.fr) / 2.0
    }

    pub fn rear_avg(&self) -> f32 {
        (self.rl + self.rr) / 2.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct WheelValuesU8 {
    pub rl: u8,
    pub rr: u8,
    pub fl: u8,
    pub fr: u8,
}

impl WheelValuesU8 {
    pub fn front_avg(&self) -> f32 {
        (self.fl as f32 + self.fr as f32) / 2.0
    }

    pub fn rear_avg(&self) -> f32 {
        (self.rl as f32 + self.rr as f32) / 2.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct WheelValuesU16 {
    pub rl: u16,
    pub rr: u16,
    pub fl: u16,
    pub fr: u16,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InputSample {
    pub session_time: f32,
    pub frame_identifier: u32,
    pub player_car_index: u8,
    pub throttle: f32,
    pub steer: f32,
    pub brake: f32,
    pub clutch: u8,
    pub speed_kmh: u16,
    pub gear: i8,
    pub rpm: u16,
    pub drs: bool,
    pub rev_lights_percent: u8,
    pub rev_lights_bit_value: u16,
    pub brake_temps_c: WheelValuesU16,
    pub tyre_surface_temps_c: WheelValuesU8,
    pub tyre_inner_temps_c: WheelValuesU8,
    pub engine_temp_c: u16,
    pub tyre_pressures_psi: WheelValuesF32,
}

impl InputSample {
    pub fn csv_header() -> &'static str {
        concat!(
            "session_time,frame_identifier,player_car_index,throttle,brake,steer,clutch,",
            "speed_kmh,gear,rpm,drs,rev_lights_percent,engine_temp_c,",
            "brake_temp_rl,brake_temp_rr,brake_temp_fl,brake_temp_fr,",
            "tyre_surface_rl,tyre_surface_rr,tyre_surface_fl,tyre_surface_fr,",
            "tyre_pressure_rl,tyre_pressure_rr,tyre_pressure_fl,tyre_pressure_fr,",
            "session_uid,session_type,session_type_name\n"
        )
    }

    pub fn to_csv_row(&self) -> String {
        self.to_csv_row_with_session(None, None)
    }

    pub fn to_csv_row_with_session(
        &self,
        session_uid: Option<u64>,
        session_type: Option<u8>,
    ) -> String {
        let session_uid = session_uid
            .map(|value| value.to_string())
            .unwrap_or_default();
        let session_type_value = session_type
            .map(|value| value.to_string())
            .unwrap_or_default();
        let session_type_name = session_type.map(f1_session_type_name).unwrap_or("");
        format!(
            concat!(
                "{:.3},{},{},{:.5},{:.5},{:.5},{},{},{},{},{},{},{},",
                "{},{},{},{},{},{},{},{},{:.3},{:.3},{:.3},{:.3},{},{},{}\n"
            ),
            self.session_time,
            self.frame_identifier,
            self.player_car_index,
            self.throttle,
            self.brake,
            self.steer,
            self.clutch,
            self.speed_kmh,
            self.gear,
            self.rpm,
            u8::from(self.drs),
            self.rev_lights_percent,
            self.engine_temp_c,
            self.brake_temps_c.rl,
            self.brake_temps_c.rr,
            self.brake_temps_c.fl,
            self.brake_temps_c.fr,
            self.tyre_surface_temps_c.rl,
            self.tyre_surface_temps_c.rr,
            self.tyre_surface_temps_c.fl,
            self.tyre_surface_temps_c.fr,
            self.tyre_pressures_psi.rl,
            self.tyre_pressures_psi.rr,
            self.tyre_pressures_psi.fl,
            self.tyre_pressures_psi.fr,
            session_uid,
            session_type_value,
            session_type_name
        )
    }
}

pub fn f1_session_type_name(value: u8) -> &'static str {
    match value {
        1..=4 => "practice",
        5..=9 => "qualifying",
        10..=14 => "sprint_qualifying",
        15..=17 => "race",
        18 => "time_trial",
        _ => "unknown",
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LapSample {
    pub session_time: f32,
    pub frame_identifier: u32,
    pub overall_frame_identifier: Option<u32>,
    pub player_car_index: u8,
    pub last_lap_time_ms: u32,
    pub current_lap_time_ms: u32,
    pub lap_distance_m: f32,
    pub total_distance_m: f32,
    pub car_position: u8,
    pub current_lap_num: u8,
    pub pit_status: u8,
    pub num_pit_stops: u8,
    pub sector: u8,
    pub current_lap_invalid: bool,
    pub driver_status: u8,
    pub result_status: u8,
    pub delta_to_car_in_front_ms: Option<u32>,
    pub car_in_front_index: Option<u8>,
    pub delta_to_car_behind_ms: Option<u32>,
    pub car_behind_index: Option<u8>,
    pub delta_to_race_leader_ms: Option<u32>,
    pub safety_car_delta_s: Option<f32>,
    pub sector1_time_ms: Option<u32>,
    pub sector2_time_ms: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RaceOrderCarSample {
    pub car_index: u8,
    pub car_position: u8,
    pub current_lap_num: u8,
    pub lap_distance_m: f32,
    pub total_distance_m: f32,
    pub last_lap_time_ms: u32,
    pub current_lap_time_ms: u32,
    pub delta_to_car_in_front_ms: Option<u32>,
    pub delta_to_race_leader_ms: Option<u32>,
    pub safety_car_delta_s: Option<f32>,
    pub pit_status: u8,
    pub num_pit_stops: u8,
    pub driver_status: u8,
    pub result_status: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RaceOrderSample {
    pub session_time: f32,
    pub frame_identifier: u32,
    pub overall_frame_identifier: Option<u32>,
    pub player_car_index: u8,
    pub cars: Vec<RaceOrderCarSample>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MarshalZoneSample {
    pub start: f32,
    pub flag: i8,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WeatherForecastSample {
    pub session_type: u8,
    pub time_offset_min: u8,
    pub weather: u8,
    pub track_temp_c: i8,
    pub track_temp_change: i8,
    pub air_temp_c: i8,
    pub air_temp_change: i8,
    pub rain_percentage: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SessionSample {
    pub session_time: f32,
    pub frame_identifier: u32,
    pub overall_frame_identifier: Option<u32>,
    pub weather: u8,
    pub total_laps: u8,
    pub track_length_m: u16,
    pub session_type: u8,
    pub track_id: i8,
    pub track_temp_c: i8,
    pub air_temp_c: i8,
    pub session_time_left_s: u16,
    pub pit_speed_limit_kmh: u8,
    pub safety_car_status: u8,
    pub marshal_zones: Vec<MarshalZoneSample>,
    pub weather_forecast_samples: Vec<WeatherForecastSample>,
    pub pit_stop_window_ideal_lap: Option<u8>,
    pub pit_stop_window_latest_lap: Option<u8>,
    pub pit_stop_rejoin_position: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DamageSample {
    pub session_time: f32,
    pub frame_identifier: u32,
    pub player_car_index: u8,
    pub tyre_wear: WheelValuesF32,
    pub tyre_damage: WheelValuesU8,
    pub tyre_blisters: WheelValuesU8,
    pub front_left_wing_damage: u8,
    pub front_right_wing_damage: u8,
    pub rear_wing_damage: u8,
    pub gearbox_damage: u8,
    pub engine_damage: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StatusSample {
    pub packet_format: Option<u16>,
    pub session_time: f32,
    pub frame_identifier: u32,
    pub player_car_index: u8,
    pub traction_control: u8,
    pub anti_lock_brakes: u8,
    pub front_brake_bias: u8,
    pub fuel_in_tank: f32,
    pub fuel_capacity: f32,
    pub fuel_delta_laps: Option<f32>,
    pub max_rpm: u16,
    pub idle_rpm: u16,
    pub max_gears: u8,
    pub drs_allowed: bool,
    pub drs_activation_distance_m: u16,
    pub pit_limiter_active: bool,
    pub actual_tyre_compound: u8,
    pub visual_tyre_compound: u8,
    pub tyres_age_laps: u8,
    pub ers_store_energy: f32,
    pub ers_deploy_mode: u8,
    pub ers_harvested_this_lap_mguk: f32,
    pub ers_harvested_this_lap_mguh: f32,
    pub ers_harvest_limit_per_lap: Option<f32>,
    pub ers_deployed_this_lap: f32,
}

impl StatusSample {
    pub fn ers_percent(&self) -> f32 {
        (self.ers_store_energy / 4_000_000.0 * 100.0).clamp(0.0, 100.0)
    }

    pub fn ers_harvested_this_lap(&self) -> f32 {
        self.ers_harvested_this_lap_mguk + self.ers_harvested_this_lap_mguh
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CarSetupSample {
    pub packet_format: u16,
    pub session_time: f32,
    pub frame_identifier: u32,
    pub player_car_index: u8,
    pub front_wing: u8,
    pub rear_wing: u8,
    pub on_throttle_differential_percent: u8,
    pub off_throttle_differential_percent: u8,
    pub front_camber: f32,
    pub rear_camber: f32,
    pub front_toe: f32,
    pub rear_toe: f32,
    pub front_suspension: u8,
    pub rear_suspension: u8,
    pub front_anti_roll_bar: u8,
    pub rear_anti_roll_bar: u8,
    pub front_ride_height: u8,
    pub rear_ride_height: u8,
    pub brake_pressure_percent: u8,
    pub brake_bias_percent: u8,
    pub engine_braking_percent: u8,
    pub tyre_pressures_psi: WheelValuesF32,
    pub ballast: u8,
    pub fuel_load_kg: f32,
    pub next_front_wing: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TyreSetInfo {
    pub index: u8,
    pub actual_tyre_compound: u8,
    pub visual_tyre_compound: u8,
    pub wear_percent: u8,
    pub available: bool,
    pub recommended_session: u8,
    pub life_span_laps: u8,
    pub usable_life_laps: u8,
    pub lap_delta_ms: i16,
    pub fitted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TyreSetsSample {
    pub packet_format: u16,
    pub session_time: f32,
    pub frame_identifier: u32,
    pub player_car_index: u8,
    pub fitted_index: u8,
    pub sets: Vec<TyreSetInfo>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FinalClassificationSample {
    pub session_time: f32,
    pub frame_identifier: u32,
    pub player_car_index: u8,
    pub position: u8,
    pub num_laps: u8,
    pub grid_position: u8,
    pub points: u8,
    pub num_pit_stops: u8,
    pub result_status: u8,
    pub result_reason: u8,
    pub best_lap_time_ms: u32,
    pub total_race_time_s: f64,
    pub penalties_time_s: u8,
    pub num_penalties: u8,
    pub num_tyre_stints: u8,
    pub tyre_stints_actual: [u8; 8],
    pub tyre_stints_visual: [u8; 8],
    pub tyre_stints_end_laps: [u8; 8],
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct TelemetryUpdate {
    pub packet_format: Option<u16>,
    pub session_uid: Option<u64>,
    pub input: Option<InputSample>,
    pub lap: Option<LapSample>,
    pub race_order: Option<RaceOrderSample>,
    pub session: Option<SessionSample>,
    pub damage: Option<DamageSample>,
    pub status: Option<StatusSample>,
    pub setup: Option<CarSetupSample>,
    pub tyre_sets: Option<TyreSetsSample>,
    pub final_classification: Option<FinalClassificationSample>,
}

impl TelemetryUpdate {
    pub fn is_empty(&self) -> bool {
        self.input.is_none()
            && self.lap.is_none()
            && self.race_order.is_none()
            && self.session.is_none()
            && self.damage.is_none()
            && self.status.is_none()
            && self.setup.is_none()
            && self.tyre_sets.is_none()
            && self.final_classification.is_none()
    }
}
