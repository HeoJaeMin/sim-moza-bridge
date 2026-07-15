use std::time::Instant;

use crate::model::{
    ImpactState, ParsedFrame, Point2, Point3, SessionState, VehicleState, VehicleTelemetry,
};

const TRACK_LENGTH_M: f64 = 5_891.0;
const PLAYER_INDEX: usize = 7;
const DEMO_SPEED: f64 = 8.0;

const DRIVERS: [&str; 24] = [
    "M. Campbell",
    "K. Estre",
    "L. Vanthoor",
    "A. Fuoco",
    "N. Nielsen",
    "M. Molina",
    "R. Kubica",
    "J. Martin",
    "Y. Ye",
    "S. Bourdais",
    "E. Bamber",
    "A. Lynn",
    "R. Frijns",
    "S. Rast",
    "R. Marciello",
    "M. Wittmann",
    "B. Hartley",
    "K. Kobayashi",
    "N. de Vries",
    "F. Makowiecki",
    "P. Hanson",
    "B. Keating",
    "S. Mann",
    "T. Milner",
];

// Simplified Silverstone Grand Prix centreline for the generated demo only.
// Live LMU sessions learn their map from shared-memory vehicle coordinates.
const TRACK_CONTROL_POINTS: [Point2; 49] = [
    Point2 { x: 72.9, z: 167.8 },
    Point2 { x: 86.5, z: 92.2 },
    Point2 { x: 99.5, z: 72.1 },
    Point2 { x: 127.7, z: 60.3 },
    Point2 { x: 193.4, z: 58.2 },
    Point2 { x: 257.7, z: 67.4 },
    Point2 { x: 312.7, z: 79.3 },
    Point2 { x: 351.4, z: 72.1 },
    Point2 { x: 372.8, z: 75.4 },
    Point2 { x: 397.2, z: 92.3 },
    Point2 { x: 421.5, z: 102.7 },
    Point2 { x: 446.5, z: 98.6 },
    Point2 { x: 466.5, z: 91.8 },
    Point2 { x: 486.0, z: 97.5 },
    Point2 { x: 497.7, z: 111.1 },
    Point2 { x: 509.1, z: 146.9 },
    Point2 { x: 530.8, z: 174.1 },
    Point2 { x: 679.6, z: 308.0 },
    Point2 { x: 752.3, z: 382.2 },
    Point2 { x: 762.4, z: 406.5 },
    Point2 { x: 754.3, z: 430.5 },
    Point2 { x: 734.1, z: 444.7 },
    Point2 { x: 686.7, z: 455.3 },
    Point2 { x: 596.5, z: 501.2 },
    Point2 { x: 590.5, z: 514.7 },
    Point2 { x: 598.5, z: 530.9 },
    Point2 { x: 589.4, z: 553.9 },
    Point2 { x: 564.7, z: 566.1 },
    Point2 { x: 535.8, z: 560.3 },
    Point2 { x: 509.0, z: 533.3 },
    Point2 { x: 388.5, z: 397.9 },
    Point2 { x: 394.6, z: 356.7 },
    Point2 { x: 420.0, z: 300.0 },
    Point2 { x: 394.3, z: 235.2 },
    Point2 { x: 370.0, z: 200.0 },
    Point2 { x: 400.0, z: 190.0 },
    Point2 { x: 420.0, z: 160.0 },
    Point2 { x: 338.5, z: 145.3 },
    Point2 { x: 130.9, z: 302.9 },
    Point2 { x: 123.0, z: 318.1 },
    Point2 { x: 135.8, z: 331.3 },
    Point2 { x: 163.0, z: 341.3 },
    Point2 { x: 173.3, z: 354.5 },
    Point2 { x: 171.6, z: 373.3 },
    Point2 { x: 156.0, z: 384.0 },
    Point2 { x: 132.7, z: 374.6 },
    Point2 { x: 81.4, z: 327.6 },
    Point2 { x: 59.3, z: 282.4 },
    Point2 { x: 72.9, z: 167.8 },
];

pub struct DemoSource {
    started: Instant,
}

impl DemoSource {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }

    pub fn frame(&self) -> ParsedFrame {
        let session_time_s = self.started.elapsed().as_secs_f64() * DEMO_SPEED + 15.0;
        let mut vehicles = Vec::with_capacity(DRIVERS.len());
        for (index, driver_name) in DRIVERS.iter().enumerate() {
            let lap_time_s = 105.0 + index as f64 * 0.18;
            let total_distance = session_time_s / lap_time_s * TRACK_LENGTH_M - index as f64 * 82.0;
            let completed_laps = (total_distance / TRACK_LENGTH_M).floor().max(0.0) as i16;
            let lap_distance_m = total_distance.rem_euclid(TRACK_LENGTH_M);
            let world = point_at_distance(lap_distance_m);
            let speed_kmh =
                242.0 + (lap_distance_m / 430.0).sin() * 68.0 + (index as f64 * 0.7).sin() * 4.0;
            let class_name = if index < 20 { "Hypercar" } else { "LMGT3" };
            vehicles.push(VehicleState {
                id: 1_000 + index as i32,
                steam_id: 76_561_198_000_000_000 + index as u64,
                driver_name: (*driver_name).to_owned(),
                vehicle_name: if index < 20 {
                    format!("Prototype #{:02}", index + 1)
                } else {
                    format!("GT3 #{:02}", index + 1)
                },
                class_name: class_name.to_owned(),
                position: (index + 1) as u8,
                completed_laps,
                lap_distance_m,
                best_lap_time_s: Some(lap_time_s - 0.9),
                last_lap_time_s: Some(lap_time_s),
                interval_s: (index > 0).then_some(0.8 + index as f64 * 0.08),
                gap_to_leader_s: (index > 0).then_some(index as f64 * 1.22),
                laps_behind_next: 0,
                laps_behind_leader: i32::from(index >= 20),
                in_pits: index == 18 && (session_time_s as i64 / 20) % 3 == 0,
                pit_state: 0,
                is_player: index == PLAYER_INDEX,
                world,
                speed_kmh,
            });
        }

        let telemetry = vehicles
            .iter()
            .enumerate()
            .map(|(index, vehicle)| demo_telemetry(vehicle, index, session_time_s))
            .collect::<Vec<_>>();
        let player = telemetry[PLAYER_INDEX].clone();

        let contact_time = (session_time_s / 30.0).floor() * 30.0;
        let impacts = if contact_time >= 30.0 {
            let position = point_at_distance((contact_time * 41.0).rem_euclid(TRACK_LENGTH_M));
            vec![
                ImpactState {
                    vehicle_id: vehicles[PLAYER_INDEX].id,
                    event_time_s: contact_time,
                    magnitude: 3.8,
                    position: Point3 {
                        x: position.x,
                        y: 0.0,
                        z: position.z,
                    },
                },
                ImpactState {
                    vehicle_id: vehicles[PLAYER_INDEX + 1].id,
                    event_time_s: contact_time + 0.03,
                    magnitude: 2.9,
                    position: Point3 {
                        x: position.x + 0.4,
                        y: 0.0,
                        z: position.z - 0.2,
                    },
                },
            ]
        } else {
            Vec::new()
        };

        ParsedFrame {
            session: SessionState {
                id: String::new(),
                game_version: 13,
                track_name: "Silverstone Grand Prix Circuit".to_owned(),
                session_type: "Race".to_owned(),
                current_time_s: session_time_s,
                time_remaining_s: (21_600.0 - session_time_s).max(0.0),
                max_laps: 0,
                track_length_m: TRACK_LENGTH_M,
                game_phase: 5,
                ambient_temp_c: 21.0,
                track_temp_c: 28.4,
                raining: 0.0,
            },
            vehicles,
            telemetry,
            player: Some(player),
            impacts,
        }
    }
}

fn demo_telemetry(
    vehicle: &VehicleState,
    vehicle_index: usize,
    session_time_s: f64,
) -> VehicleTelemetry {
    let lap_time_s = 105.0 + vehicle_index as f64 * 0.18;
    let phase = vehicle.lap_distance_m / TRACK_LENGTH_M * std::f64::consts::TAU;
    let braking = ((phase * 5.0 + vehicle_index as f64 * 0.03).sin().max(0.0)).powf(7.0);
    let throttle = (0.92 - braking * 0.85 + (phase * 2.0).sin() * 0.05).clamp(0.0, 1.0);
    VehicleTelemetry {
        vehicle_id: vehicle.id,
        lap_number: i32::from(vehicle.completed_laps) + 1,
        lap_distance_m: vehicle.lap_distance_m,
        lap_elapsed_s: vehicle.lap_distance_m / TRACK_LENGTH_M * lap_time_s,
        session_time_s,
        speed_kmh: vehicle.speed_kmh,
        rpm: 5_500.0 + throttle * 3_900.0 + phase.sin() * 350.0,
        gear: ((vehicle.speed_kmh / 48.0).round() as i32).clamp(1, 7),
        throttle,
        brake: braking,
        steer: (phase * 3.0).sin() * 0.48 + (phase * 7.0).sin() * 0.08,
        clutch: 0.0,
        lateral_g: (phase * 3.0).sin() * 1.8,
        longitudinal_g: throttle * 0.8 - braking * 2.4,
        world: vehicle.world,
        lap_invalidated: false,
    }
}

fn point_at_distance(distance_m: f64) -> Point2 {
    let segment_lengths = TRACK_CONTROL_POINTS
        .windows(2)
        .map(|points| distance(points[0], points[1]))
        .collect::<Vec<_>>();
    let total_length = segment_lengths.iter().sum::<f64>();
    let mut remaining = distance_m.rem_euclid(TRACK_LENGTH_M) / TRACK_LENGTH_M * total_length;
    for (index, segment_length) in segment_lengths.iter().copied().enumerate() {
        if remaining <= segment_length {
            let fraction = if segment_length <= f64::EPSILON {
                0.0
            } else {
                remaining / segment_length
            };
            let start = TRACK_CONTROL_POINTS[index];
            let end = TRACK_CONTROL_POINTS[index + 1];
            return Point2 {
                x: start.x + (end.x - start.x) * fraction,
                z: start.z + (end.z - start.z) * fraction,
            };
        }
        remaining -= segment_length;
    }
    TRACK_CONTROL_POINTS[0]
}

fn distance(left: Point2, right: Point2) -> f64 {
    let dx = right.x - left.x;
    let dz = right.z - left.z;
    dx.mul_add(dx, dz * dz).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_a_complete_demo_grid() {
        let source = DemoSource::new();
        let frame = source.frame();
        assert_eq!(frame.vehicles.len(), 24);
        assert_eq!(frame.telemetry.len(), 24);
        assert!(frame.vehicles[PLAYER_INDEX].is_player);
        assert_eq!(frame.player.as_ref().unwrap().vehicle_id, 1_007);
        assert_eq!(frame.telemetry[0].vehicle_id, 1_000);
        assert!(frame.telemetry[0].rpm > 0.0);
        assert_eq!(frame.session.track_length_m, TRACK_LENGTH_M);
    }
}
