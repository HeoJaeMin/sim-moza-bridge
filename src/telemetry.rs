#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Copy, Debug, PartialEq)]
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelValuesU16 {
    pub rl: u16,
    pub rr: u16,
    pub fl: u16,
    pub fr: u16,
}

#[derive(Clone, Debug, PartialEq)]
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
            "tyre_pressure_rl,tyre_pressure_rr,tyre_pressure_fl,tyre_pressure_fr\n"
        )
    }

    pub fn to_csv_row(&self) -> String {
        format!(
            concat!(
                "{:.3},{},{},{:.5},{:.5},{:.5},{},{},{},{},{},{},{},",
                "{},{},{},{},{},{},{},{},{:.3},{:.3},{:.3},{:.3}\n"
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
            self.tyre_pressures_psi.fr
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LapSample {
    pub session_time: f32,
    pub frame_identifier: u32,
    pub player_car_index: u8,
    pub last_lap_time_ms: u32,
    pub current_lap_time_ms: u32,
    pub lap_distance_m: f32,
    pub total_distance_m: f32,
    pub car_position: u8,
    pub current_lap_num: u8,
    pub pit_status: u8,
    pub sector: u8,
    pub current_lap_invalid: bool,
    pub driver_status: u8,
    pub result_status: u8,
    pub delta_to_car_in_front_ms: Option<u32>,
    pub delta_to_car_behind_ms: Option<u32>,
    pub delta_to_race_leader_ms: Option<u32>,
    pub sector1_time_ms: Option<u32>,
    pub sector2_time_ms: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SessionSample {
    pub session_time: f32,
    pub frame_identifier: u32,
    pub total_laps: u8,
    pub track_length_m: u16,
    pub session_type: u8,
    pub track_id: i8,
    pub track_temp_c: i8,
    pub air_temp_c: i8,
    pub session_time_left_s: u16,
}

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
pub struct StatusSample {
    pub session_time: f32,
    pub frame_identifier: u32,
    pub player_car_index: u8,
    pub traction_control: u8,
    pub anti_lock_brakes: u8,
    pub front_brake_bias: u8,
    pub fuel_in_tank: f32,
    pub fuel_capacity: f32,
    pub fuel_remaining_laps: f32,
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
    pub ers_deployed_this_lap: f32,
}

impl StatusSample {
    pub fn ers_percent(&self) -> f32 {
        (self.ers_store_energy / 4_000_000.0 * 100.0).clamp(0.0, 100.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TelemetryUpdate {
    pub input: Option<InputSample>,
    pub lap: Option<LapSample>,
    pub session: Option<SessionSample>,
    pub damage: Option<DamageSample>,
    pub status: Option<StatusSample>,
}

impl TelemetryUpdate {
    pub fn is_empty(&self) -> bool {
        self.input.is_none()
            && self.lap.is_none()
            && self.session.is_none()
            && self.damage.is_none()
            && self.status.is_none()
    }
}
