use std::sync::{Arc, Mutex};

use crate::telemetry::{
    CarSetupSample, DamageSample, FinalClassificationSample, InputSample, LapSample,
    RaceOrderSample, SessionSample, StatusSample, TelemetryUpdate, TyreSetsSample,
};

#[derive(Clone)]
pub struct HudHandle {
    state: Arc<Mutex<HudState>>,
}

impl HudHandle {
    pub fn update(&self, update: &TelemetryUpdate) {
        if let Ok(mut state) = self.state.lock() {
            state.apply(update);
        }
    }

    pub fn snapshot(&self) -> TelemetryUpdate {
        self.state
            .lock()
            .map(|state| state.snapshot())
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Default)]
struct HudState {
    packet_format: Option<u16>,
    session_uid: Option<u64>,
    input: Option<InputSample>,
    lap: Option<LapSample>,
    race_order: Option<RaceOrderSample>,
    session: Option<SessionSample>,
    damage: Option<DamageSample>,
    status: Option<StatusSample>,
    setup: Option<CarSetupSample>,
    tyre_sets: Option<TyreSetsSample>,
    final_classification: Option<FinalClassificationSample>,
}

impl HudState {
    fn apply(&mut self, update: &TelemetryUpdate) {
        if update.packet_format.is_some() {
            self.packet_format = update.packet_format;
        }
        if update.session_uid.is_some() {
            self.session_uid = update.session_uid;
        }
        if let Some(input) = &update.input {
            self.input = Some(input.clone());
        }
        if let Some(lap) = &update.lap {
            self.lap = Some(lap.clone());
        }
        if let Some(race_order) = &update.race_order {
            self.race_order = Some(race_order.clone());
        }
        if let Some(session) = &update.session {
            self.session = Some(session.clone());
        }
        if let Some(damage) = &update.damage {
            self.damage = Some(damage.clone());
        }
        if let Some(status) = &update.status {
            self.status = Some(status.clone());
        }
        if let Some(setup) = &update.setup {
            self.setup = Some(setup.clone());
        }
        if let Some(tyre_sets) = &update.tyre_sets {
            self.tyre_sets = Some(tyre_sets.clone());
        }
        if let Some(final_classification) = &update.final_classification {
            self.final_classification = Some(final_classification.clone());
        }
    }

    fn snapshot(&self) -> TelemetryUpdate {
        TelemetryUpdate {
            packet_format: self.packet_format,
            session_uid: self.session_uid,
            input: self.input.clone(),
            lap: self.lap.clone(),
            race_order: self.race_order.clone(),
            session: self.session.clone(),
            damage: self.damage.clone(),
            status: self.status.clone(),
            setup: self.setup.clone(),
            tyre_sets: self.tyre_sets.clone(),
            final_classification: self.final_classification.clone(),
        }
    }
}

pub fn new_hud_handle() -> HudHandle {
    HudHandle {
        state: Arc::new(Mutex::new(HudState::default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::{WheelValuesF32, WheelValuesU8, WheelValuesU16};

    #[test]
    fn merges_partial_updates_into_snapshot() {
        let handle = new_hud_handle();
        let input = InputSample {
            session_time: 12.5,
            frame_identifier: 42,
            player_car_index: 0,
            throttle: 0.75,
            steer: -0.1,
            brake: 0.2,
            clutch: 0,
            speed_kmh: 210,
            gear: 6,
            rpm: 11_500,
            drs: true,
            rev_lights_percent: 70,
            rev_lights_bit_value: 1,
            brake_temps_c: WheelValuesU16 {
                fl: 101,
                fr: 102,
                rl: 103,
                rr: 104,
            },
            tyre_surface_temps_c: WheelValuesU8 {
                fl: 81,
                fr: 82,
                rl: 83,
                rr: 84,
            },
            tyre_inner_temps_c: WheelValuesU8 {
                fl: 91,
                fr: 92,
                rl: 93,
                rr: 94,
            },
            engine_temp_c: 100,
            tyre_pressures_psi: WheelValuesF32 {
                fl: 22.1,
                fr: 22.2,
                rl: 21.9,
                rr: 22.0,
            },
        };
        let damage = DamageSample {
            session_time: 13.0,
            frame_identifier: 43,
            player_car_index: 0,
            tyre_wear: WheelValuesF32 {
                fl: 10.0,
                fr: 11.0,
                rl: 12.0,
                rr: 13.0,
            },
            tyre_damage: WheelValuesU8 {
                fl: 1,
                fr: 2,
                rl: 3,
                rr: 4,
            },
            tyre_blisters: WheelValuesU8 {
                fl: 5,
                fr: 6,
                rl: 7,
                rr: 8,
            },
            front_left_wing_damage: 9,
            front_right_wing_damage: 10,
            rear_wing_damage: 11,
            gearbox_damage: 12,
            engine_damage: 13,
        };

        handle.update(&TelemetryUpdate {
            input: Some(input.clone()),
            ..TelemetryUpdate::default()
        });
        handle.update(&TelemetryUpdate {
            damage: Some(damage.clone()),
            ..TelemetryUpdate::default()
        });

        let snapshot = handle.snapshot();
        assert_eq!(snapshot.input, Some(input));
        assert_eq!(snapshot.damage, Some(damage));
        assert_eq!(snapshot.status, None);
    }
}
