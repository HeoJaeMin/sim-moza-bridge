use super::*;
    use crate::bridge::BridgeMode;
    use crate::games::AUTO;
    use crate::telemetry::{
        FinalClassificationSample, MarshalZoneSample, RaceOrderCarSample, WheelValuesU8,
        WheelValuesU16,
    };

    fn driving_input(session_time: f32) -> InputSample {
        InputSample {
            session_time,
            frame_identifier: session_time as u32,
            player_car_index: 0,
            throttle: 0.7,
            steer: 0.0,
            brake: 0.0,
            clutch: 0,
            speed_kmh: 180,
            gear: 5,
            rpm: 10_000,
            drs: false,
            rev_lights_percent: 50,
            rev_lights_bit_value: 0,
            brake_temps_c: WheelValuesU16 {
                rl: 0,
                rr: 0,
                fl: 0,
                fr: 0,
            },
            tyre_surface_temps_c: WheelValuesU8 {
                rl: 0,
                rr: 0,
                fl: 0,
                fr: 0,
            },
            tyre_inner_temps_c: WheelValuesU8 {
                rl: 0,
                rr: 0,
                fl: 0,
                fr: 0,
            },
            engine_temp_c: 0,
            tyre_pressures_psi: WheelValuesF32 {
                rl: 0.0,
                rr: 0.0,
                fl: 0.0,
                fr: 0.0,
            },
        }
    }

    fn lap(lap_num: u8, front_gap_ms: u32) -> LapSample {
        LapSample {
            session_time: 10.0,
            frame_identifier: 1,
            overall_frame_identifier: None,
            player_car_index: 0,
            last_lap_time_ms: 90_123,
            current_lap_time_ms: 10_000,
            lap_distance_m: 1_000.0,
            total_distance_m: 1_000.0,
            car_position: 3,
            current_lap_num: lap_num,
            pit_status: 0,
            num_pit_stops: 0,
            sector: 0,
            current_lap_invalid: false,
            driver_status: 4,
            result_status: 2,
            delta_to_car_in_front_ms: Some(front_gap_ms),
            car_in_front_index: Some(1),
            delta_to_car_behind_ms: Some(2_000),
            car_behind_index: Some(4),
            delta_to_race_leader_ms: Some(3_000),
            safety_car_delta_s: Some(0.0),
            sector1_time_ms: None,
            sector2_time_ms: None,
        }
    }

    fn lap_at(session_time: f32, lap_num: u8, front_gap_ms: u32) -> LapSample {
        let mut sample = lap(lap_num, front_gap_ms);
        sample.session_time = session_time;
        sample
    }

    fn practice_status(tyres_age_laps: u8) -> StatusSample {
        StatusSample {
            packet_format: Some(2026),
            session_time: 10.0,
            frame_identifier: 1,
            player_car_index: 0,
            traction_control: 0,
            anti_lock_brakes: 0,
            front_brake_bias: 56,
            fuel_in_tank: 20.0,
            fuel_capacity: 110.0,
            fuel_delta_laps: Some(5.0),
            max_rpm: 13_000,
            idle_rpm: 4_000,
            max_gears: 9,
            drs_allowed: false,
            drs_activation_distance_m: 0,
            pit_limiter_active: false,
            actual_tyre_compound: 20,
            visual_tyre_compound: 18,
            tyres_age_laps,
            ers_store_energy: 4_000_000.0,
            ers_deploy_mode: 1,
            ers_harvested_this_lap_mguk: 0.0,
            ers_harvested_this_lap_mguh: 0.0,
            ers_harvest_limit_per_lap: Some(9_000_000.0),
            ers_deployed_this_lap: 0.0,
        }
    }

    fn race_session(total_laps: u8) -> SessionSample {
        SessionSample {
            session_time: 10.0,
            frame_identifier: 1,
            overall_frame_identifier: None,
            weather: 0,
            total_laps,
            track_length_m: 5_408,
            session_type: 15,
            track_id: 3,
            track_temp_c: 32,
            air_temp_c: 24,
            session_time_left_s: 5_000,
            pit_speed_limit_kmh: 80,
            safety_car_status: 0,
            marshal_zones: Vec::new(),
            weather_forecast_samples: Vec::new(),
            pit_stop_window_ideal_lap: None,
            pit_stop_window_latest_lap: None,
            pit_stop_rejoin_position: None,
        }
    }

    fn completed_race_lap(
        lap_num: u8,
        lap_time_ms: u32,
        tyre_age_laps: u8,
        max_wear: f32,
        fuel_delta_laps: f32,
        ers_percent: f32,
    ) -> CompletedLapAnalysis {
        let mut status = practice_status(tyre_age_laps);
        status.visual_tyre_compound = 17;
        status.actual_tyre_compound = 18;
        status.fuel_delta_laps = Some(fuel_delta_laps);
        status.ers_store_energy = ers_percent.clamp(0.0, 100.0) * 40_000.0;
        CompletedLapAnalysis {
            session_uid: Some(42),
            session_type: Some(15),
            lap_num,
            lap_time_ms,
            clean: true,
            invalid_reason: None,
            track_length_m: 5_408.0,
            sample_count: 5_000,
            corners: Vec::new(),
            recommendations: Vec::new(),
            latest_damage: Some(DamageSample {
                session_time: lap_num as f32 * 90.0,
                frame_identifier: lap_num as u32,
                player_car_index: 0,
                tyre_wear: WheelValuesF32 {
                    rl: max_wear,
                    rr: (max_wear - 4.0).max(0.0),
                    fl: (max_wear - 7.0).max(0.0),
                    fr: (max_wear - 10.0).max(0.0),
                },
                tyre_damage: WheelValuesU8 {
                    rl: max_wear as u8,
                    rr: (max_wear - 4.0).max(0.0) as u8,
                    fl: (max_wear - 7.0).max(0.0) as u8,
                    fr: (max_wear - 10.0).max(0.0) as u8,
                },
                tyre_blisters: WheelValuesU8 {
                    rl: 0,
                    rr: 0,
                    fl: 0,
                    fr: 0,
                },
                front_left_wing_damage: 0,
                front_right_wing_damage: 0,
                rear_wing_damage: 0,
                gearbox_damage: 0,
                engine_damage: 0,
            }),
            latest_status: Some(status),
            latest_setup: None,
        }
    }

    fn race_order_car(car_index: u8, position: u8, pit_status: u8) -> RaceOrderCarSample {
        RaceOrderCarSample {
            car_index,
            car_position: position,
            current_lap_num: 12,
            lap_distance_m: 2_000.0,
            total_distance_m: 60_000.0,
            last_lap_time_ms: 93_000,
            current_lap_time_ms: 45_000,
            delta_to_car_in_front_ms: Some(2_000),
            delta_to_race_leader_ms: Some(4_000),
            safety_car_delta_s: None,
            pit_status,
            num_pit_stops: u8::from(pit_status != 0),
            driver_status: 1,
            result_status: 2,
        }
    }

    fn car_setup(on_throttle_differential_percent: u8) -> CarSetupSample {
        CarSetupSample {
            packet_format: 2026,
            session_time: 10.0,
            frame_identifier: 1,
            player_car_index: 0,
            front_wing: 25,
            rear_wing: 25,
            on_throttle_differential_percent,
            off_throttle_differential_percent: 40,
            front_camber: -3.5,
            rear_camber: -2.0,
            front_toe: 0.04,
            rear_toe: 0.15,
            front_suspension: 37,
            rear_suspension: 16,
            front_anti_roll_bar: 15,
            rear_anti_roll_bar: 8,
            front_ride_height: 25,
            rear_ride_height: 52,
            brake_pressure_percent: 100,
            brake_bias_percent: 57,
            engine_braking_percent: 50,
            tyre_pressures_psi: WheelValuesF32 {
                rl: 21.2,
                rr: 21.2,
                fl: 24.2,
                fr: 24.2,
            },
            ballast: 6,
            fuel_load_kg: 20.0,
            next_front_wing: 25.0,
        }
    }

    #[test]
    fn maps_f1_session_types_to_engineer_modes() {
        assert_eq!(session_type_name(1), "practice");
        assert_eq!(session_type_name(5), "qualifying");
        assert_eq!(session_type_name(10), "sprint_qualifying");
        assert_eq!(session_type_name(15), "race");
        assert_eq!(session_type_name(18), "time_trial");
    }

    #[test]
    fn practice_program_continues_an_existing_used_tyre_stint() {
        let advisor = PracticeAdvisor::default();
        let snapshot = EngineerSnapshot {
            session: Some(SessionSample {
                session_time: 10.0,
                frame_identifier: 1,
                overall_frame_identifier: None,
                weather: 0,
                total_laps: 0,
                track_length_m: 5_400,
                session_type: 1,
                track_id: 3,
                track_temp_c: 43,
                air_temp_c: 30,
                session_time_left_s: 1_800,
                pit_speed_limit_kmh: 80,
                safety_car_status: 0,
                marshal_zones: Vec::new(),
                weather_forecast_samples: Vec::new(),
                pit_stop_window_ideal_lap: None,
                pit_stop_window_latest_lap: None,
                pit_stop_rejoin_position: None,
            }),
            status: Some(practice_status(7)),
            ..EngineerSnapshot::default()
        };

        let plan = advisor.plan(&snapshot).unwrap();
        assert_eq!(plan.phase, "race_stint");
        assert_eq!(plan.target_timed_laps, 3);
        assert!(plan.objective.contains("롱런"));
    }

    #[test]
    fn p2_starts_with_a_five_lap_race_simulation() {
        let advisor = PracticeAdvisor::default();
        let snapshot = EngineerSnapshot {
            session: Some(SessionSample {
                session_time: 10.0,
                frame_identifier: 1,
                overall_frame_identifier: None,
                weather: 0,
                total_laps: 0,
                track_length_m: 5_400,
                session_type: 2,
                track_id: 3,
                track_temp_c: 43,
                air_temp_c: 30,
                session_time_left_s: 1_800,
                pit_speed_limit_kmh: 80,
                safety_car_status: 0,
                marshal_zones: Vec::new(),
                weather_forecast_samples: Vec::new(),
                pit_stop_window_ideal_lap: None,
                pit_stop_window_latest_lap: None,
                pit_stop_rejoin_position: None,
            }),
            status: Some(practice_status(0)),
            ..EngineerSnapshot::default()
        };

        let plan = advisor.plan(&snapshot).unwrap();
        assert_eq!(plan.phase, "race_stint");
        assert_eq!(plan.target_timed_laps, 5);
        assert!(plan.objective.contains("P2"));
        assert!(
            plan.instructions
                .iter()
                .any(|item| item.contains("레이스 연료"))
        );
    }

    #[test]
    fn program_announcement_ignores_only_the_remaining_lap_count() {
        let mut program = PracticeProgram {
            phase: "race_stint",
            objective: "P2 미디엄 레이스 시뮬레이션".to_owned(),
            target_timed_laps: 5,
            completed_clean_laps: 0,
            current_setup_clean_laps: 0,
            instructions: vec!["레이스 페이스 유지".to_owned()],
            basis: Vec::new(),
            setup_candidate: None,
            recent_laps: Vec::new(),
        };
        let initial = ProgramAnnouncementKey::from_program(Some(7), &program);
        program.target_timed_laps = 1;
        assert_eq!(
            initial,
            ProgramAnnouncementKey::from_program(Some(7), &program)
        );

        program.instructions[0] = "온스로틀 디퍼렌셜 50에서 45".to_owned();
        assert_ne!(
            initial,
            ProgramAnnouncementKey::from_program(Some(7), &program)
        );
    }

    #[test]
    fn practice_history_survives_packets_that_arrive_before_session_data() {
        let mut advisor = PracticeAdvisor {
            session_uid: Some(7),
            laps: vec![PracticeLapRecord {
                lap_num: 1,
                lap_time_ms: 92_000,
                clean: true,
                tyre_compound: Some(17),
                tyre_age_laps: Some(1),
                fuel_kg: Some(18.0),
                setup_signature: None,
            }],
            latest_setup_candidate: None,
            last_announced_program: None,
        };

        assert!(!advisor.sync_session(Some(7), &EngineerSnapshot::default()));
        assert_eq!(advisor.laps.len(), 1);
    }

    #[test]
    fn completed_setup_validation_waits_for_review_instead_of_chaining_changes() {
        let baseline = setup_signature(&car_setup(50));
        let changed = setup_signature(&car_setup(45));
        let mut laps = Vec::new();
        for lap_num in 1..=5 {
            laps.push(PracticeLapRecord {
                lap_num,
                lap_time_ms: 92_000,
                clean: true,
                tyre_compound: Some(17),
                tyre_age_laps: Some(lap_num - 1),
                fuel_kg: Some(20.0),
                setup_signature: Some(baseline.clone()),
            });
        }
        for lap_num in 6..=7 {
            laps.push(PracticeLapRecord {
                lap_num,
                lap_time_ms: 92_000,
                clean: true,
                tyre_compound: Some(17),
                tyre_age_laps: Some(lap_num - 1),
                fuel_kg: Some(20.0),
                setup_signature: Some(changed.clone()),
            });
        }
        let advisor = PracticeAdvisor {
            session_uid: Some(7),
            laps,
            latest_setup_candidate: Some(SetupRecommendation {
                area: "Corner exit traction".to_owned(),
                reason: "rear limited".to_owned(),
                action: "온스로틀 디퍼렌셜 50에서 45".to_owned(),
                confidence: "high".to_owned(),
            }),
            last_announced_program: None,
        };
        let snapshot = EngineerSnapshot {
            session: Some(SessionSample {
                session_time: 10.0,
                frame_identifier: 1,
                overall_frame_identifier: None,
                weather: 0,
                total_laps: 0,
                track_length_m: 5_400,
                session_type: 2,
                track_id: 3,
                track_temp_c: 30,
                air_temp_c: 24,
                session_time_left_s: 1_800,
                pit_speed_limit_kmh: 80,
                safety_car_status: 0,
                marshal_zones: Vec::new(),
                weather_forecast_samples: Vec::new(),
                pit_stop_window_ideal_lap: None,
                pit_stop_window_latest_lap: None,
                pit_stop_rejoin_position: None,
            }),
            status: Some(practice_status(7)),
            setup: Some(car_setup(45)),
            ..EngineerSnapshot::default()
        };

        let plan = advisor.plan(&snapshot).unwrap();
        assert_eq!(plan.phase, "setup_review");
        assert_eq!(plan.target_timed_laps, 0);
    }

    #[test]
    fn normal_wear_mirrored_in_tyre_damage_is_not_a_damage_signal() {
        let damage = DamageSample {
            session_time: 1.0,
            frame_identifier: 1,
            player_car_index: 0,
            tyre_wear: WheelValuesF32 {
                rl: 15.1,
                rr: 13.2,
                fl: 13.6,
                fr: 8.7,
            },
            tyre_damage: WheelValuesU8 {
                rl: 15,
                rr: 13,
                fl: 13,
                fr: 8,
            },
            tyre_blisters: WheelValuesU8 {
                rl: 0,
                rr: 0,
                fl: 0,
                fr: 0,
            },
            front_left_wing_damage: 0,
            front_right_wing_damage: 0,
            rear_wing_damage: 0,
            gearbox_damage: 0,
            engine_damage: 0,
        };

        assert_eq!(max_excess_tyre_damage(&damage), 0.0);
    }

    #[test]
    fn stays_quiet_until_the_car_is_driving() {
        let mut core = EngineerCore::default();
        let mut input = driving_input(1.0);
        input.speed_kmh = 0;
        input.rpm = 0;
        input.throttle = 0.0;
        input.gear = 0;

        assert!(
            core.ingest(&TelemetryUpdate {
                input: Some(input),
                ..TelemetryUpdate::default()
            })
            .is_empty()
        );

        let calls = core.ingest(&TelemetryUpdate {
            input: Some(driving_input(2.0)),
            ..TelemetryUpdate::default()
        });
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].kind, "engineer_online");
    }

    #[test]
    fn announces_gap_only_once_until_it_clears() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });

        let candidate = core.ingest(&TelemetryUpdate {
            lap: Some(lap_at(10.0, 1, 900)),
            ..TelemetryUpdate::default()
        });
        assert!(!candidate.iter().any(|call| call.kind == "front_gap"));

        let first = core.ingest(&TelemetryUpdate {
            lap: Some(lap_at(11.0, 1, 850)),
            ..TelemetryUpdate::default()
        });
        assert!(first.iter().any(|call| call.kind == "front_gap"));

        let repeated = core.ingest(&TelemetryUpdate {
            lap: Some(lap_at(11.2, 1, 800)),
            ..TelemetryUpdate::default()
        });
        assert!(!repeated.iter().any(|call| call.kind == "front_gap"));

        core.ingest(&TelemetryUpdate {
            lap: Some(lap_at(12.0, 1, 1_600)),
            ..TelemetryUpdate::default()
        });
        core.ingest(&TelemetryUpdate {
            lap: Some(lap_at(13.0, 1, 1_600)),
            ..TelemetryUpdate::default()
        });
        let second_candidate = core.ingest(&TelemetryUpdate {
            lap: Some(lap_at(13.1, 1, 700)),
            ..TelemetryUpdate::default()
        });
        assert!(!second_candidate.iter().any(|call| call.kind == "front_gap"));
        let again = core.ingest(&TelemetryUpdate {
            lap: Some(lap_at(14.1, 1, 700)),
            ..TelemetryUpdate::default()
        });
        assert!(again.iter().any(|call| call.kind == "front_gap"));
    }

    #[test]
    fn does_not_rearm_a_confirmed_gap_from_one_large_sample() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });

        core.ingest(&TelemetryUpdate {
            lap: Some(lap_at(20.0, 1, 800)),
            ..TelemetryUpdate::default()
        });
        assert!(
            core.ingest(&TelemetryUpdate {
                lap: Some(lap_at(21.0, 1, 800)),
                ..TelemetryUpdate::default()
            })
            .iter()
            .any(|call| call.kind == "front_gap")
        );

        core.ingest(&TelemetryUpdate {
            lap: Some(lap_at(21.2, 1, 3_000)),
            ..TelemetryUpdate::default()
        });
        assert!(
            core.ingest(&TelemetryUpdate {
                lap: Some(lap_at(21.3, 1, 700)),
                ..TelemetryUpdate::default()
            })
            .iter()
            .all(|call| call.kind != "front_gap")
        );
    }

    #[test]
    fn rejects_the_recorded_transient_behind_gap_spike() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });

        let mut stable = lap_at(4_776.633, 51, 0);
        stable.car_position = 1;
        stable.delta_to_car_behind_ms = Some(20_900);
        assert!(
            core.ingest(&TelemetryUpdate {
                lap: Some(stable),
                ..TelemetryUpdate::default()
            })
            .iter()
            .all(|call| call.kind != "behind_gap")
        );

        for (session_time, gap_ms) in [(4_776.682, 50), (4_776.9, 50), (4_777.1, 21_150)] {
            let mut sample = lap_at(session_time, 51, 0);
            sample.car_position = 1;
            sample.delta_to_car_behind_ms = Some(gap_ms);
            assert!(
                core.ingest(&TelemetryUpdate {
                    lap: Some(sample),
                    ..TelemetryUpdate::default()
                })
                .iter()
                .all(|call| call.kind != "behind_gap")
            );
        }
    }

    #[test]
    fn rejects_the_recorded_point_eight_four_second_behind_gap_spike() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });

        for (session_time, gap_ms) in [
            (935.6147, 2_717),
            (935.6287, 34),
            (935.8327, 34),
            (936.0511, 34),
            (936.26245, 34),
            (936.4688, 34),
            (936.6808, 2_750),
        ] {
            let mut sample = lap_at(session_time, 10, 0);
            sample.car_position = 1;
            sample.delta_to_car_behind_ms = Some(gap_ms);
            assert!(
                core.ingest(&TelemetryUpdate {
                    lap: Some(sample),
                    ..TelemetryUpdate::default()
                })
                .iter()
                .all(|call| call.kind != "behind_gap")
            );
        }
    }

    #[test]
    fn restarts_behind_gap_confirmation_when_the_peer_changes() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });

        let mut first = lap_at(20.0, 5, 2_000);
        first.delta_to_car_behind_ms = Some(800);
        first.car_behind_index = Some(4);
        assert!(
            core.ingest(&TelemetryUpdate {
                lap: Some(first),
                ..TelemetryUpdate::default()
            })
            .is_empty()
        );

        let mut changed = lap_at(20.8, 5, 2_000);
        changed.delta_to_car_behind_ms = Some(700);
        changed.car_behind_index = Some(5);
        assert!(
            core.ingest(&TelemetryUpdate {
                lap: Some(changed),
                ..TelemetryUpdate::default()
            })
            .is_empty()
        );

        let mut too_soon = lap_at(21.1, 5, 2_000);
        too_soon.delta_to_car_behind_ms = Some(650);
        too_soon.car_behind_index = Some(5);
        assert!(
            core.ingest(&TelemetryUpdate {
                lap: Some(too_soon),
                ..TelemetryUpdate::default()
            })
            .iter()
            .all(|call| call.kind != "behind_gap")
        );

        let mut confirmed = lap_at(21.8, 5, 2_000);
        confirmed.delta_to_car_behind_ms = Some(600);
        confirmed.car_behind_index = Some(5);
        assert!(
            core.ingest(&TelemetryUpdate {
                lap: Some(confirmed),
                ..TelemetryUpdate::default()
            })
            .iter()
            .any(|call| call.kind == "behind_gap")
        );
    }

    #[test]
    fn announces_a_sustained_behind_gap_after_confirmation() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });

        let mut first = lap_at(20.0, 5, 2_000);
        first.delta_to_car_behind_ms = Some(900);
        assert!(
            core.ingest(&TelemetryUpdate {
                lap: Some(first),
                ..TelemetryUpdate::default()
            })
            .iter()
            .all(|call| call.kind != "behind_gap")
        );

        let mut confirmed = lap_at(21.0, 5, 2_000);
        confirmed.delta_to_car_behind_ms = Some(800);
        assert!(
            core.ingest(&TelemetryUpdate {
                lap: Some(confirmed),
                ..TelemetryUpdate::default()
            })
            .iter()
            .any(|call| call.kind == "behind_gap")
        );
    }

    #[test]
    fn announces_invalid_laps_without_guessing_completion_from_lap_packets() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });
        core.ingest(&TelemetryUpdate {
            lap: Some(lap(1, 2_000)),
            ..TelemetryUpdate::default()
        });

        let mut next_lap = lap(2, 2_000);
        next_lap.current_lap_invalid = true;
        let calls = core.ingest(&TelemetryUpdate {
            lap: Some(next_lap),
            ..TelemetryUpdate::default()
        });

        assert!(!calls.iter().any(|call| call.kind == "lap_complete"));
        assert!(calls.iter().any(|call| call.kind == "lap_invalid"));
    }

    #[test]
    fn warns_about_damage() {
        let mut core = EngineerCore {
            damage_initialized: true,
            ..EngineerCore::default()
        };
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });
        let damage_calls = core.ingest(&TelemetryUpdate {
            damage: Some(DamageSample {
                session_time: 12.0,
                frame_identifier: 4,
                player_car_index: 0,
                tyre_wear: WheelValuesF32 {
                    rl: 20.0,
                    rr: 20.0,
                    fl: 75.0,
                    fr: 20.0,
                },
                tyre_damage: WheelValuesU8 {
                    rl: 0,
                    rr: 0,
                    fl: 0,
                    fr: 0,
                },
                tyre_blisters: WheelValuesU8 {
                    rl: 0,
                    rr: 0,
                    fl: 0,
                    fr: 0,
                },
                front_left_wing_damage: 40,
                front_right_wing_damage: 0,
                rear_wing_damage: 0,
                gearbox_damage: 0,
                engine_damage: 0,
            }),
            ..TelemetryUpdate::default()
        });
        assert!(damage_calls.iter().any(|call| call.kind == "tyre_wear"));
        assert!(
            damage_calls
                .iter()
                .any(|call| call.kind == "front_wing_damage")
        );
    }

    #[test]
    fn reports_yellow_and_green_flags_for_the_current_zone() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });
        let session = SessionSample {
            session_time: 2.0,
            frame_identifier: 2,
            overall_frame_identifier: None,
            weather: 0,
            total_laps: 10,
            track_length_m: 5_000,
            session_type: 15,
            track_id: 1,
            track_temp_c: 30,
            air_temp_c: 20,
            session_time_left_s: 1_000,
            pit_speed_limit_kmh: 80,
            safety_car_status: 0,
            marshal_zones: vec![
                MarshalZoneSample {
                    start: 0.0,
                    flag: 1,
                },
                MarshalZoneSample {
                    start: 0.2,
                    flag: 3,
                },
                MarshalZoneSample {
                    start: 0.5,
                    flag: 1,
                },
            ],
            weather_forecast_samples: Vec::new(),
            pit_stop_window_ideal_lap: None,
            pit_stop_window_latest_lap: None,
            pit_stop_rejoin_position: None,
        };
        core.ingest(&TelemetryUpdate {
            session: Some(session),
            ..TelemetryUpdate::default()
        });

        let mut yellow_lap = lap(1, 2_000);
        yellow_lap.lap_distance_m = 1_500.0;
        let yellow = core.ingest(&TelemetryUpdate {
            lap: Some(yellow_lap),
            ..TelemetryUpdate::default()
        });
        assert!(yellow.iter().any(|call| call.kind == "yellow_flag"));

        let mut green_lap = lap(1, 2_000);
        green_lap.lap_distance_m = 3_000.0;
        let green = core.ingest(&TelemetryUpdate {
            lap: Some(green_lap),
            ..TelemetryUpdate::default()
        });
        assert!(green.iter().any(|call| call.kind == "green_flag"));
    }

    #[test]
    fn does_not_treat_a_marshal_zone_blue_flag_as_player_specific() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });
        core.ingest(&TelemetryUpdate {
            session: Some(SessionSample {
                session_time: 2.0,
                frame_identifier: 2,
                overall_frame_identifier: None,
                weather: 0,
                total_laps: 10,
                track_length_m: 5_000,
                session_type: 15,
                track_id: 1,
                track_temp_c: 30,
                air_temp_c: 20,
                session_time_left_s: 1_000,
                pit_speed_limit_kmh: 80,
                safety_car_status: 0,
                marshal_zones: vec![MarshalZoneSample {
                    start: 0.0,
                    flag: 2,
                }],
                weather_forecast_samples: Vec::new(),
                pit_stop_window_ideal_lap: None,
                pit_stop_window_latest_lap: None,
                pit_stop_rejoin_position: None,
            }),
            ..TelemetryUpdate::default()
        });
        let calls = core.ingest(&TelemetryUpdate {
            lap: Some(lap(1, 2_000)),
            ..TelemetryUpdate::default()
        });

        assert!(!calls.iter().any(|call| call.kind == "blue_flag"));
    }

    #[test]
    fn race_strategy_replaces_modulo_five_chatter_with_log_only_snapshots() {
        let mut advisor = RaceStrategyAdvisor::default();
        let mut status = practice_status(9);
        status.fuel_delta_laps = Some(0.6);
        status.visual_tyre_compound = 16;
        status.ers_store_energy = 2_000_000.0;
        let snapshot = EngineerSnapshot {
            lap: Some(lap(10, 2_000)),
            session: Some(SessionSample {
                session_time: 900.0,
                frame_identifier: 10,
                overall_frame_identifier: None,
                weather: 0,
                total_laps: 57,
                track_length_m: 5_408,
                session_type: 15,
                track_id: 3,
                track_temp_c: 32,
                air_temp_c: 24,
                session_time_left_s: 5_000,
                pit_speed_limit_kmh: 80,
                safety_car_status: 0,
                marshal_zones: Vec::new(),
                weather_forecast_samples: Vec::new(),
                pit_stop_window_ideal_lap: None,
                pit_stop_window_latest_lap: None,
                pit_stop_rejoin_position: None,
            }),
            status: Some(status),
            ..EngineerSnapshot::default()
        };

        let completed = CompletedLapAnalysis {
            session_uid: Some(42),
            session_type: Some(15),
            lap_num: 10,
            lap_time_ms: 94_410,
            clean: true,
            invalid_reason: None,
            track_length_m: 5_408.0,
            sample_count: 5_000,
            corners: Vec::new(),
            recommendations: Vec::new(),
            latest_damage: None,
            latest_status: snapshot.status.clone(),
            latest_setup: None,
        };

        let calls = advisor.observe(&completed, &snapshot);
        assert!(calls.iter().all(|call| call.kind != "race_stint_update"));
        assert!(
            calls
                .iter()
                .any(|call| call.kind == "race_strategy_snapshot")
        );
    }

    #[test]
    fn detects_sustained_bahrain_soft_degradation_once() {
        let mut advisor = RaceStrategyAdvisor::default();
        let mut snapshot = EngineerSnapshot {
            session_uid: Some(42),
            session: Some(race_session(57)),
            ..EngineerSnapshot::default()
        };
        let mut calls = Vec::new();
        for lap_num in 2..=14 {
            snapshot.lap = Some(lap(lap_num, 2_000));
            let completed = completed_race_lap(
                lap_num,
                94_000 + (lap_num as u32 - 2) * 155,
                lap_num - 1,
                12.0 + (lap_num as f32 - 2.0) * 2.7,
                0.6,
                45.0,
            );
            calls.extend(advisor.observe(&completed, &snapshot));
        }

        let degradation = calls
            .iter()
            .filter(|call| call.kind == "tyre_degradation")
            .collect::<Vec<_>>();
        assert_eq!(degradation.len(), 1);
        assert!(degradation[0].message.contains("+0.16초"));
        assert!(degradation[0].message.contains("왼쪽 뒤"));
    }

    #[test]
    fn bahrain_medium_outlier_is_ignored_and_late_stop_is_vetoed() {
        let mut advisor = RaceStrategyAdvisor::default();
        let mut session = race_session(57);
        session.pit_stop_window_ideal_lap = Some(55);
        session.pit_stop_window_latest_lap = Some(55);
        session.pit_stop_rejoin_position = Some(3);
        let mut snapshot = EngineerSnapshot {
            session_uid: Some(42),
            session: Some(session),
            ..EngineerSnapshot::default()
        };
        let mut calls = Vec::new();
        snapshot.lap = Some(lap(2, 2_000));
        let mut opening_soft = completed_race_lap(2, 94_000, 1, 12.0, 0.8, 50.0);
        if let Some(status) = &mut opening_soft.latest_status {
            status.visual_tyre_compound = 18;
        }
        calls.extend(advisor.observe(&opening_soft, &snapshot));
        for lap_num in 40..=54 {
            let mut current_lap = lap(lap_num, 10_000);
            current_lap.car_position = 1;
            current_lap.num_pit_stops = 2;
            snapshot.lap = Some(current_lap);
            let outlier = if lap_num == 48 { 1_900 } else { 0 };
            let completed = completed_race_lap(
                lap_num,
                92_500 + (lap_num as u32 - 40) * 60 + outlier,
                lap_num - 39,
                10.0 + (lap_num as f32 - 40.0) * (24.35 / 14.0),
                0.7,
                35.0,
            );
            calls.extend(advisor.observe(&completed, &snapshot));
        }

        assert!(calls.iter().all(|call| call.kind != "tyre_degradation"));
        assert_eq!(
            calls
                .iter()
                .filter(|call| call.kind == "strategy_stay_out")
                .count(),
            1
        );
        assert!(
            calls
                .iter()
                .all(|call| !matches!(call.kind, "pit_window_open" | "pit_window_latest"))
        );
        let summary = advisor.summary(&snapshot).expect("race strategy summary");
        assert!(summary.projected_finish_wear_percent.unwrap() < 40.0);
        assert!(summary.pace_trend_s_per_lap.unwrap() < 0.08);

        let mut next_lap = lap(55, 10_000);
        next_lap.car_position = 1;
        next_lap.num_pit_stops = 2;
        snapshot.lap = Some(next_lap);
        let completed = completed_race_lap(55, 93_400, 16, 36.1, 0.6, 30.0);
        let next_calls = advisor.observe(&completed, &snapshot);
        assert!(
            next_calls
                .iter()
                .all(|call| !matches!(call.kind, "pit_window_open" | "pit_window_latest"))
        );

        snapshot.session.as_mut().unwrap().safety_car_status = 1;
        assert_eq!(
            advisor
                .reassess_live_conditions(&snapshot)
                .map(|call| call.kind),
            Some("strategy_reassess")
        );
    }
