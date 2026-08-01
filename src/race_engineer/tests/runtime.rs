    #[test]
    fn stay_out_requires_compound_obligation_and_clear_forecast() {
        fn observe_endgame(
            advisor: &mut RaceStrategyAdvisor,
            snapshot: &mut EngineerSnapshot,
        ) -> Vec<EngineerCall> {
            let mut calls = Vec::new();
            for lap_num in 50..=54 {
                let mut current_lap = lap(lap_num, 5_000);
                current_lap.car_position = 1;
                current_lap.num_pit_stops = 1;
                snapshot.lap = Some(current_lap);
                calls.extend(advisor.observe(
                    &completed_race_lap(
                        lap_num,
                        93_000 + (lap_num as u32 - 50) * 40,
                        lap_num - 49,
                        20.0 + (lap_num as f32 - 50.0) * 1.5,
                        0.5,
                        40.0,
                    ),
                    snapshot,
                ));
            }
            calls
        }

        let mut session = race_session(57);
        session.pit_stop_rejoin_position = Some(3);
        let mut snapshot = EngineerSnapshot {
            session_uid: Some(42),
            session: Some(session.clone()),
            ..EngineerSnapshot::default()
        };
        let no_obligation = observe_endgame(&mut RaceStrategyAdvisor::default(), &mut snapshot);
        assert!(
            no_obligation
                .iter()
                .all(|call| call.kind != "strategy_stay_out")
        );

        session.weather_forecast_samples = vec![crate::telemetry::WeatherForecastSample {
            session_type: 15,
            time_offset_min: 5,
            weather: 3,
            track_temp_c: 28,
            track_temp_change: -1,
            air_temp_c: 22,
            air_temp_change: 0,
            rain_percentage: 60,
        }];
        let mut rainy_snapshot = EngineerSnapshot {
            session_uid: Some(42),
            session: Some(session),
            ..EngineerSnapshot::default()
        };
        let mut rainy_advisor = RaceStrategyAdvisor::default();
        rainy_snapshot.lap = Some(lap(2, 2_000));
        let mut soft = completed_race_lap(2, 94_000, 1, 10.0, 0.8, 50.0);
        soft.latest_status.as_mut().unwrap().visual_tyre_compound = 18;
        rainy_advisor.observe(&soft, &rainy_snapshot);
        let rain_risk = observe_endgame(&mut rainy_advisor, &mut rainy_snapshot);
        assert!(
            rain_risk
                .iter()
                .all(|call| call.kind != "strategy_stay_out")
        );
    }

    #[test]
    fn new_stint_rearms_a_changed_pit_window() {
        let mut advisor = RaceStrategyAdvisor::default();
        let mut session = race_session(57);
        session.pit_stop_window_ideal_lap = Some(3);
        session.pit_stop_window_latest_lap = Some(4);
        let mut snapshot = EngineerSnapshot {
            session_uid: Some(42),
            session: Some(session.clone()),
            lap: Some(lap(2, 3_000)),
            ..EngineerSnapshot::default()
        };
        let mut first_stint = completed_race_lap(2, 93_000, 1, 12.0, 0.8, 50.0);
        first_stint
            .latest_status
            .as_mut()
            .unwrap()
            .visual_tyre_compound = 18;
        let first = advisor.observe(&first_stint, &snapshot);

        session.pit_stop_window_ideal_lap = Some(6);
        session.pit_stop_window_latest_lap = Some(7);
        snapshot.session = Some(session);
        snapshot.lap = Some(lap(5, 3_000));
        let mut second_stint = completed_race_lap(5, 93_000, 1, 8.0, 0.7, 45.0);
        second_stint
            .latest_status
            .as_mut()
            .unwrap()
            .visual_tyre_compound = 17;
        let second = advisor.observe(&second_stint, &snapshot);

        assert!(first.iter().any(|call| call.kind == "pit_window_open"));
        assert!(second.iter().any(|call| call.kind == "pit_window_open"));
    }

    #[test]
    fn ers_target_requires_three_low_clean_laps_and_repeats_only_after_recovery() {
        let mut advisor = RaceStrategyAdvisor::default();
        let mut snapshot = EngineerSnapshot {
            session_uid: Some(42),
            session: Some(race_session(57)),
            ..EngineerSnapshot::default()
        };
        let ers_by_lap = [
            (2, 50.0),
            (3, 0.0),
            (4, 50.0),
            (5, 5.0),
            (6, 4.0),
            (7, 3.0),
            (8, 2.0),
        ];
        let mut calls = Vec::new();
        for (lap_num, ers) in ers_by_lap {
            snapshot.lap = Some(lap(lap_num, 2_000));
            let completed = completed_race_lap(lap_num, 93_000, lap_num - 1, 20.0, 0.5, ers);
            calls.extend(advisor.observe(&completed, &snapshot));
        }

        assert_eq!(
            calls
                .iter()
                .filter(|call| call.kind == "ers_target")
                .count(),
            1
        );
        assert!(
            calls
                .iter()
                .find(|call| call.kind == "ers_target")
                .is_some_and(|call| call.message.contains("20% 회복"))
        );
    }

    #[test]
    fn fuel_target_needs_two_laps_while_positive_margin_stays_silent() {
        let mut advisor = RaceStrategyAdvisor::default();
        let mut snapshot = EngineerSnapshot {
            session_uid: Some(42),
            session: Some(race_session(57)),
            ..EngineerSnapshot::default()
        };
        let fuel_by_lap = [(2, 0.8), (3, 0.6), (4, -0.4), (5, -0.5), (6, -0.6)];
        let mut calls = Vec::new();
        for (lap_num, fuel) in fuel_by_lap {
            snapshot.lap = Some(lap(lap_num, 2_000));
            let completed = completed_race_lap(lap_num, 93_000, lap_num - 1, 18.0, fuel, 40.0);
            calls.extend(advisor.observe(&completed, &snapshot));
        }

        assert_eq!(
            calls
                .iter()
                .filter(|call| call.kind == "fuel_target")
                .count(),
            1
        );
    }

    #[test]
    fn safety_car_is_prioritized_and_suppresses_combat_gap_calls() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });
        let mut session = race_session(57);
        session.session_time = 2.0;
        core.ingest(&TelemetryUpdate {
            session: Some(session.clone()),
            ..TelemetryUpdate::default()
        });

        session.session_time = 3.0;
        session.safety_car_status = 1;
        let race_control = core.ingest(&TelemetryUpdate {
            session: Some(session),
            ..TelemetryUpdate::default()
        });
        assert!(race_control.iter().any(|call| call.kind == "safety_car"));

        let mut close = lap_at(4.5, 3, 500);
        close.delta_to_car_behind_ms = Some(500);
        let gap_calls = core.ingest(&TelemetryUpdate {
            lap: Some(close),
            ..TelemetryUpdate::default()
        });
        assert!(
            gap_calls
                .iter()
                .all(|call| !matches!(call.kind, "front_gap" | "behind_gap"))
        );
    }

    #[test]
    fn stale_session_packet_does_not_restart_or_repeat_safety_car() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });
        let mut safety_car = race_session(57);
        safety_car.session_time = 10.0;
        safety_car.frame_identifier = 100;
        safety_car.safety_car_status = 1;
        let deployed = core.ingest(&TelemetryUpdate {
            session: Some(safety_car.clone()),
            ..TelemetryUpdate::default()
        });

        let mut stale_green = safety_car.clone();
        stale_green.session_time = 9.5;
        stale_green.frame_identifier = 99;
        stale_green.safety_car_status = 0;
        let stale = core.ingest(&TelemetryUpdate {
            session: Some(stale_green),
            ..TelemetryUpdate::default()
        });

        safety_car.session_time = 10.1;
        safety_car.frame_identifier = 101;
        let repeated = core.ingest(&TelemetryUpdate {
            session: Some(safety_car),
            ..TelemetryUpdate::default()
        });

        assert_eq!(
            deployed
                .iter()
                .filter(|call| call.kind == "safety_car")
                .count(),
            1
        );
        assert!(stale.iter().all(|call| call.kind != "race_restart"));
        assert!(repeated.iter().all(|call| call.kind != "safety_car"));
        assert_eq!(core.snapshot.session.as_ref().unwrap().safety_car_status, 1);
    }

    #[test]
    fn overall_frame_distinguishes_flashback_from_delayed_lap_packet() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(100.0)),
            ..TelemetryUpdate::default()
        });
        let mut original = lap_at(100.0, 10, 2_000);
        original.frame_identifier = 100;
        original.overall_frame_identifier = Some(100);
        core.ingest(&TelemetryUpdate {
            lap: Some(original),
            ..TelemetryUpdate::default()
        });

        let mut flashback = lap_at(90.0, 9, 2_000);
        flashback.frame_identifier = 90;
        flashback.overall_frame_identifier = Some(101);
        let rewound = core.ingest_with_context(&TelemetryUpdate {
            lap: Some(flashback),
            ..TelemetryUpdate::default()
        });
        assert!(rewound.timeline_reset.is_some());
        assert_eq!(core.snapshot.lap.as_ref().unwrap().session_time, 90.0);

        let mut delayed = lap_at(89.0, 9, 500);
        delayed.frame_identifier = 89;
        delayed.overall_frame_identifier = Some(99);
        let stale = core.ingest_with_context(&TelemetryUpdate {
            lap: Some(delayed),
            ..TelemetryUpdate::default()
        });
        assert!(stale.timeline_reset.is_none());
        assert_eq!(core.snapshot.lap.as_ref().unwrap().session_time, 90.0);
    }

    #[test]
    fn safety_car_stored_before_driving_is_announced_on_start() {
        let mut core = EngineerCore::default();
        let mut session = race_session(57);
        session.safety_car_status = 1;
        let waiting = core.ingest(&TelemetryUpdate {
            session: Some(session),
            ..TelemetryUpdate::default()
        });
        let started = core.ingest(&TelemetryUpdate {
            input: Some(driving_input(11.0)),
            ..TelemetryUpdate::default()
        });

        assert!(waiting.is_empty());
        assert!(started.iter().any(|call| call.kind == "safety_car"));
    }

    #[test]
    fn safety_car_laps_do_not_build_resource_or_degradation_warnings() {
        let mut advisor = RaceStrategyAdvisor::default();
        let mut session = race_session(57);
        session.safety_car_status = 1;
        session.pit_stop_window_ideal_lap = Some(3);
        session.pit_stop_window_latest_lap = Some(4);
        let mut snapshot = EngineerSnapshot {
            session_uid: Some(42),
            session: Some(session),
            ..EngineerSnapshot::default()
        };
        let mut calls = Vec::new();
        for lap_num in 2..=8 {
            snapshot.lap = Some(lap(lap_num, 500));
            let completed = completed_race_lap(
                lap_num,
                94_000 + (lap_num as u32 - 2) * 300,
                lap_num - 1,
                15.0 + lap_num as f32 * 2.0,
                -0.8,
                5.0,
            );
            calls.extend(advisor.observe(&completed, &snapshot));
        }

        assert!(calls.is_empty());
    }

    #[test]
    fn announces_a_nearby_rival_pit_entry_once() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            session: Some(race_session(57)),
            ..TelemetryUpdate::default()
        });
        let mut player_lap = lap_at(10.0, 12, 2_000);
        player_lap.car_position = 2;
        player_lap.car_in_front_index = None;
        player_lap.car_behind_index = None;
        let baseline = RaceOrderSample {
            session_time: 10.0,
            frame_identifier: 10,
            overall_frame_identifier: None,
            player_car_index: 0,
            cars: vec![
                race_order_car(0, 2, 0),
                race_order_car(1, 1, 0),
                race_order_car(2, 3, 0),
            ],
        };
        core.ingest(&TelemetryUpdate {
            lap: Some(player_lap),
            race_order: Some(baseline.clone()),
            ..TelemetryUpdate::default()
        });

        let mut in_lap = baseline;
        in_lap.session_time = 10.5;
        in_lap.frame_identifier = 11;
        in_lap.cars[1].driver_status = 2;
        core.ingest(&TelemetryUpdate {
            race_order: Some(in_lap.clone()),
            ..TelemetryUpdate::default()
        });

        let mut rival_pits = in_lap;
        rival_pits.session_time = 11.0;
        rival_pits.frame_identifier = 12;
        rival_pits.cars[1].pit_status = 1;
        let first = core.ingest(&TelemetryUpdate {
            race_order: Some(rival_pits.clone()),
            ..TelemetryUpdate::default()
        });
        let repeated = core.ingest(&TelemetryUpdate {
            race_order: Some(rival_pits),
            ..TelemetryUpdate::default()
        });

        assert_eq!(
            first
                .iter()
                .filter(|call| call.kind == "rival_pit_front")
                .count(),
            1
        );
        assert!(repeated.iter().all(|call| call.kind != "rival_pit_front"));
    }

    #[test]
    fn distant_lapped_or_non_race_rival_pit_is_silent() {
        fn pit_transition(
            core: &mut EngineerCore,
            mut session: SessionSample,
        ) -> Vec<EngineerCall> {
            core.ingest(&TelemetryUpdate {
                input: Some(driving_input(1.0)),
                session: Some(session.clone()),
                ..TelemetryUpdate::default()
            });
            let mut player_lap = lap_at(10.0, 12, 2_000);
            player_lap.car_position = 2;
            player_lap.delta_to_car_behind_ms = Some(40_000);
            let mut behind = race_order_car(2, 3, 0);
            behind.current_lap_num = 11;
            behind.delta_to_car_in_front_ms = Some(40_000);
            let baseline = RaceOrderSample {
                session_time: 10.0,
                frame_identifier: 10,
                overall_frame_identifier: None,
                player_car_index: 0,
                cars: vec![race_order_car(0, 2, 0), race_order_car(1, 1, 0), behind],
            };
            core.ingest(&TelemetryUpdate {
                lap: Some(player_lap),
                race_order: Some(baseline.clone()),
                ..TelemetryUpdate::default()
            });
            let mut pitting = baseline;
            pitting.session_time = 11.0;
            pitting.frame_identifier = 11;
            pitting.cars[2].pit_status = 1;
            pitting.cars[2].num_pit_stops = 1;
            session.session_time = 11.0;
            core.ingest(&TelemetryUpdate {
                session: Some(session),
                race_order: Some(pitting),
                ..TelemetryUpdate::default()
            })
        }

        let distant = pit_transition(&mut EngineerCore::default(), race_session(57));
        assert!(
            distant
                .iter()
                .all(|call| !call.kind.starts_with("rival_pit"))
        );

        let mut practice = race_session(57);
        practice.session_type = 2;
        let non_race = pit_transition(&mut EngineerCore::default(), practice);
        assert!(
            non_race
                .iter()
                .all(|call| !call.kind.starts_with("rival_pit"))
        );
    }

    #[test]
    fn final_classification_finishes_even_without_a_driving_snapshot() {
        let mut core = EngineerCore::default();
        let final_classification = FinalClassificationSample {
            session_time: 5_400.0,
            frame_identifier: 90_000,
            player_car_index: 0,
            position: 1,
            num_laps: 53,
            grid_position: 4,
            points: 25,
            num_pit_stops: 1,
            result_status: 3,
            result_reason: 2,
            best_lap_time_ms: 89_000,
            total_race_time_s: 5_300.0,
            penalties_time_s: 0,
            num_penalties: 0,
            num_tyre_stints: 2,
            tyre_stints_actual: [0; 8],
            tyre_stints_visual: [0; 8],
            tyre_stints_end_laps: [0; 8],
        };

        let first = core.ingest(&TelemetryUpdate {
            final_classification: Some(final_classification.clone()),
            ..TelemetryUpdate::default()
        });
        let repeated = core.ingest(&TelemetryUpdate {
            final_classification: Some(final_classification),
            ..TelemetryUpdate::default()
        });

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].kind, "session_finished");
        assert!(repeated.is_empty());
        assert!(!core.driving);
    }

    #[test]
    fn lifecycle_maps_final_classification_results() {
        let base = FinalClassificationSample {
            session_time: 5_400.0,
            frame_identifier: 90_000,
            player_car_index: 0,
            position: 1,
            num_laps: 53,
            grid_position: 4,
            points: 25,
            num_pit_stops: 1,
            result_status: 3,
            result_reason: 2,
            best_lap_time_ms: 89_000,
            total_race_time_s: 5_300.0,
            penalties_time_s: 0,
            num_penalties: 0,
            num_tyre_stints: 2,
            tyre_stints_actual: [0; 8],
            tyre_stints_visual: [0; 8],
            tyre_stints_end_laps: [0; 8],
        };

        for (result_status, expected) in [
            (3, SessionLifecycleStatus::Finished),
            (4, SessionLifecycleStatus::DidNotFinish),
            (5, SessionLifecycleStatus::Disqualified),
            (6, SessionLifecycleStatus::NotClassified),
            (7, SessionLifecycleStatus::DidNotFinish),
            (2, SessionLifecycleStatus::Ended),
        ] {
            let mut classification = base.clone();
            classification.result_status = result_status;
            let lifecycle = SessionLifecycle::from_final_classification(&classification);
            assert_eq!(lifecycle.status, expected);
            assert_eq!(lifecycle.end_reason, Some("final_classification"));
            assert!(lifecycle.ended_at_unix_ms.is_some());
        }
    }

    #[test]
    fn writes_an_event_trigger_immediately_when_driving_starts() {
        let unique = format!("sim-moza-trigger-{}-{}", std::process::id(), unix_ms());
        let trigger_path = std::env::temp_dir().join(format!("{unique}.json"));
        let state_path = std::env::temp_dir().join(format!("{unique}-state.json"));
        let history_path = std::env::temp_dir().join(format!("{unique}.jsonl"));
        let event_path = std::env::temp_dir().join(format!("{unique}-events.jsonl"));
        let config = BridgeConfig {
            game: AUTO,
            listen_host: "127.0.0.1".to_owned(),
            listen_port: 20777,
            moza_host: "127.0.0.1".to_owned(),
            moza_port: 22025,
            mode: BridgeMode::Remap,
            fix_tyre_wear_order: false,
            f1_24_car_damage_compat: true,
            input_log: None,
            corner_log: None,
            analysis_report: None,
            race_engineer: true,
            engineer_voice: false,
            engineer_log: Some(event_path.to_string_lossy().into_owned()),
            engineer_state: Some(state_path.to_string_lossy().into_owned()),
            engineer_history: Some(history_path.to_string_lossy().into_owned()),
            engineer_trigger: Some(trigger_path.to_string_lossy().into_owned()),
            engineer_hook: None,
            engineer_ai_hook: None,
            engineer_ai_task_id: None,
            engineer_radio_hook: None,
            dry_run: false,
            debug: false,
        };
        let mut engineer = RaceEngineer::open(&config).unwrap().unwrap();

        engineer.ingest(
            "f1-25",
            &TelemetryUpdate {
                packet_format: Some(2026),
                session_uid: Some(42),
                input: Some(driving_input(100.0)),
                ..TelemetryUpdate::default()
            },
            None,
        );

        let trigger: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&trigger_path).expect("event trigger should be written"),
        )
        .unwrap();
        assert_eq!(trigger["schema_version"], 4);
        assert_eq!(trigger["decision_mode"], "rules");
        assert_eq!(trigger["state"]["decision_mode"], "rules");
        assert_eq!(trigger["state"]["schema_version"], 8);
        assert!(trigger["state"]["radio_revisions"]["track_flag"].is_number());
        assert!(trigger["state"]["radio_revisions"]["conditions"].is_number());
        assert!(trigger["state"]["radio_revisions"]["pit"].is_number());
        assert_eq!(trigger["reasons"][0], "engineer_online");
        assert_eq!(trigger["state"]["packet_format"], 2026);
        assert_eq!(trigger["state"]["session_uid"], 42);
        assert!(
            std::fs::read_to_string(&history_path)
                .unwrap()
                .contains("\"schema_version\":8")
        );

        engineer.ingest(
            "f1-25",
            &TelemetryUpdate {
                packet_format: Some(2026),
                session_uid: Some(42),
                input: Some(driving_input(90.0)),
                ..TelemetryUpdate::default()
            },
            None,
        );

        let trigger: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&trigger_path).expect("timeline reset trigger should be written"),
        )
        .unwrap();
        assert_eq!(trigger["schema_version"], 4);
        assert_eq!(trigger["decision_mode"], "rules");
        assert_eq!(trigger["reasons"][0], "timeline_reset");
        assert_eq!(trigger["timeline_revision"], 1);
        assert_eq!(
            trigger["timeline_reset"]["rollback_from_session_time"],
            100.0
        );
        assert_eq!(trigger["timeline_reset"]["rollback_to_session_time"], 90.0);
        let event_log = std::fs::read_to_string(&event_path).unwrap();
        let reset_event: serde_json::Value = serde_json::from_str(
            event_log
                .lines()
                .last()
                .expect("timeline reset event should be appended"),
        )
        .unwrap();
        assert_eq!(reset_event["schema_version"], 2);
        assert_eq!(reset_event["kind"], "timeline_reset");
        assert_eq!(reset_event["timeline_revision"], 1);
        assert_eq!(reset_event["timeline_reset"]["revision"], 1);

        engineer.finish_session("bridge_shutdown");
        engineer.finish_session("bridge_shutdown");
        let state: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&state_path).expect("terminal state should be written"),
        )
        .unwrap();
        assert_eq!(state["schema_version"], 8);
        assert_eq!(state["lifecycle"]["status"], "interrupted");
        assert_eq!(state["lifecycle"]["end_reason"], "bridge_shutdown");
        assert_eq!(state["driving"], false);
        let event_log = std::fs::read_to_string(&event_path).unwrap();
        assert_eq!(
            event_log
                .lines()
                .filter(|line| line.contains("\"kind\":\"session_interrupted\""))
                .count(),
            1
        );

        drop(engineer);
        let _ = std::fs::remove_file(trigger_path);
        let _ = std::fs::remove_file(state_path);
        let _ = std::fs::remove_file(history_path);
        let _ = std::fs::remove_file(event_path);
    }

    #[test]
    fn bridge_restart_in_same_session_does_not_reannounce_online() {
        let unique = format!("sim-moza-resume-{}-{}", std::process::id(), unix_ms());
        let trigger_path = std::env::temp_dir().join(format!("{unique}.json"));
        let state_path = std::env::temp_dir().join(format!("{unique}-state.json"));
        let event_path = std::env::temp_dir().join(format!("{unique}-events.jsonl"));
        std::fs::write(&state_path, br#"{"session_uid":42}"#).unwrap();

        let config = BridgeConfig {
            game: AUTO,
            listen_host: "127.0.0.1".to_owned(),
            listen_port: 20777,
            moza_host: "127.0.0.1".to_owned(),
            moza_port: 22025,
            mode: BridgeMode::Remap,
            fix_tyre_wear_order: false,
            f1_24_car_damage_compat: true,
            input_log: None,
            corner_log: None,
            analysis_report: None,
            race_engineer: true,
            engineer_voice: false,
            engineer_log: Some(event_path.to_string_lossy().into_owned()),
            engineer_state: Some(state_path.to_string_lossy().into_owned()),
            engineer_history: None,
            engineer_trigger: Some(trigger_path.to_string_lossy().into_owned()),
            engineer_hook: None,
            engineer_ai_hook: None,
            engineer_ai_task_id: None,
            engineer_radio_hook: None,
            dry_run: false,
            debug: false,
        };
        let mut engineer = RaceEngineer::open(&config).unwrap().unwrap();

        engineer.ingest(
            "f1-25",
            &TelemetryUpdate {
                packet_format: Some(2026),
                session_uid: Some(42),
                input: Some(driving_input(101.0)),
                ..TelemetryUpdate::default()
            },
            None,
        );

        assert!(std::fs::read_to_string(&event_path).unwrap().is_empty());
        assert!(!trigger_path.exists());

        drop(engineer);
        let _ = std::fs::remove_file(trigger_path);
        let _ = std::fs::remove_file(state_path);
        let _ = std::fs::remove_file(event_path);
    }

    #[test]
    fn detects_a_same_session_flashback_and_preserves_the_online_announcement() {
        let mut core = EngineerCore::default();
        let first = core.ingest_with_context(&TelemetryUpdate {
            session_uid: Some(42),
            input: Some(driving_input(100.0)),
            ..TelemetryUpdate::default()
        });
        assert!(
            first
                .calls
                .iter()
                .any(|call| call.kind == "engineer_online")
        );

        let rewound = core.ingest_with_context(&TelemetryUpdate {
            session_uid: Some(42),
            input: Some(driving_input(90.0)),
            ..TelemetryUpdate::default()
        });

        let reset = rewound
            .timeline_reset
            .expect("flashback should be detected");
        assert_eq!(reset.revision, 1);
        assert_eq!(reset.session_uid, Some(42));
        assert_eq!(reset.rollback_from_session_time, 100.0);
        assert_eq!(reset.rollback_to_session_time, 90.0);
        assert!(
            rewound
                .calls
                .iter()
                .any(|call| call.kind == "timeline_reset")
        );
        assert!(
            !rewound
                .calls
                .iter()
                .any(|call| call.kind == "engineer_online")
        );
        assert_eq!(core.timeline_revision, 1);
        assert_eq!(
            core.snapshot.input.as_ref().map(|input| input.session_time),
            Some(90.0)
        );
    }

    #[test]
    fn stationary_resume_does_not_reannounce_online_but_a_new_uid_does() {
        let mut core = EngineerCore::default();
        let first = core.ingest(&TelemetryUpdate {
            session_uid: Some(42),
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });
        assert!(first.iter().any(|call| call.kind == "engineer_online"));

        core.last_motion_at = Some(Instant::now() - Duration::from_secs(16));
        let mut stopped = driving_input(2.0);
        stopped.speed_kmh = 0;
        stopped.rpm = 0;
        stopped.throttle = 0.0;
        stopped.gear = 0;
        assert!(
            core.ingest(&TelemetryUpdate {
                session_uid: Some(42),
                input: Some(stopped),
                ..TelemetryUpdate::default()
            })
            .is_empty()
        );
        assert!(!core.driving);

        let resumed = core.ingest(&TelemetryUpdate {
            session_uid: Some(42),
            input: Some(driving_input(3.0)),
            ..TelemetryUpdate::default()
        });
        assert!(!resumed.iter().any(|call| call.kind == "engineer_online"));

        let new_session = core.ingest(&TelemetryUpdate {
            session_uid: Some(43),
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });
        assert!(
            new_session
                .iter()
                .any(|call| call.kind == "engineer_online")
        );
    }

    fn voice_job(kind: &'static str, message: &str, queued_at_unix_ms: u128) -> VoiceJob {
        VoiceJob {
            queued_at_unix_ms,
            source: "f1-25".to_owned(),
            session_uid: Some(42),
            timeline_revision: 0,
            state_revision: 0,
            session_type: Some(15),
            lap: Some(3),
            position: Some(2),
            priority: CallPriority::Important,
            kind,
            message: message.to_owned(),
        }
    }

    #[test]
    fn voice_queue_keeps_only_the_latest_transient_state() {
        let mut mailbox = PendingVoiceJobs::default();
        mailbox.synchronize(RadioScope {
            source_key: stable_source_key("f1-25"),
            session_uid: Some(42),
            safe_to_speak: true,
            ..RadioScope::default()
        });
        assert!(mailbox.enqueue(voice_job("position", "현재 P3.", 1)));
        assert!(mailbox.enqueue(voice_job("yellow_flag", "옐로 플래그.", 2)));
        assert!(mailbox.enqueue(voice_job("position", "현재 P2.", 3)));
        assert!(mailbox.enqueue(voice_job("green_flag", "그린 플래그.", 4)));

        assert_eq!(mailbox.pending.len(), 2);
        assert!(mailbox.pending.iter().any(|job| job.message == "현재 P2."));
        assert!(
            mailbox
                .pending
                .iter()
                .any(|job| job.message == "그린 플래그.")
        );
    }

    #[test]
    fn voice_mailbox_keeps_the_latest_state_and_prioritizes_new_critical_calls() {
        let mut mailbox = PendingVoiceJobs::default();
        mailbox.synchronize(RadioScope {
            source_key: stable_source_key("f1-25"),
            session_uid: Some(42),
            safe_to_speak: true,
            ..RadioScope::default()
        });
        for (index, kind) in [
            "position",
            "front_gap",
            "behind_gap",
            "lap_invalid",
            "pit_limiter",
            "tyre_wear",
            "tyre_damage",
            "front_wing_damage",
            "rear_wing_damage",
            "gearbox_damage",
            "engine_damage",
            "practice_program",
            "session_finished",
        ]
        .into_iter()
        .enumerate()
        {
            assert!(mailbox.enqueue(voice_job(kind, kind, index as u128 + 1)));
        }
        assert!(mailbox.enqueue(voice_job("position", "최신 P1", 20)));
        let mut critical = voice_job("yellow_flag", "옐로 플래그", 21);
        critical.priority = CallPriority::Critical;
        assert!(mailbox.enqueue(critical));

        assert_eq!(
            mailbox
                .pending
                .iter()
                .filter(|job| job.kind == "position")
                .count(),
            1
        );
        assert!(mailbox.pending.iter().any(|job| job.message == "최신 P1"));
        assert_eq!(mailbox.take_next().map(|job| job.kind), Some("yellow_flag"));
    }

    #[test]
    fn simultaneous_critical_damage_calls_are_all_queued() {
        let mut mailbox = PendingVoiceJobs::default();
        mailbox.synchronize(RadioScope {
            source_key: stable_source_key("f1-25"),
            session_uid: Some(42),
            safe_to_speak: true,
            ..RadioScope::default()
        });
        for kind in ["tyre_damage", "engine_damage"] {
            let mut job = voice_job(kind, kind, 1);
            job.priority = CallPriority::Critical;
            assert!(mailbox.enqueue(job));
        }

        assert_eq!(mailbox.pending.len(), 2);
        assert_eq!(
            mailbox.take_next().unwrap().priority,
            CallPriority::Critical
        );
        assert_eq!(
            mailbox.take_next().unwrap().priority,
            CallPriority::Critical
        );
    }

    #[test]
    fn unsafe_driving_zone_defers_strategy_but_not_critical_calls() {
        let mut mailbox = PendingVoiceJobs::default();
        mailbox.synchronize(RadioScope {
            source_key: stable_source_key("f1-25"),
            session_uid: Some(42),
            safe_to_speak: false,
            ..RadioScope::default()
        });
        assert!(mailbox.enqueue(voice_job("ers_target", "ERS 관리", 1)));
        let mut critical = voice_job("safety_car", "세이프티 카", 2);
        critical.priority = CallPriority::Critical;
        assert!(mailbox.enqueue(critical));

        assert_eq!(mailbox.take_next().map(|job| job.kind), Some("safety_car"));
        assert!(mailbox.take_next().is_none());
        mailbox.synchronize(RadioScope {
            source_key: stable_source_key("f1-25"),
            session_uid: Some(42),
            safe_to_speak: true,
            ..RadioScope::default()
        });
        assert_eq!(mailbox.take_next().map(|job| job.kind), Some("ers_target"));
    }

    #[test]
    fn voice_mailbox_invalidates_gap_position_flag_and_timeline_state() {
        let mut mailbox = PendingVoiceJobs::default();
        let mut revisions = RadioStateRevisions {
            position: 1,
            front_gap: 1,
            behind_gap: 1,
            track_flag: 1,
            race_control: 1,
            ..RadioStateRevisions::default()
        };
        mailbox.synchronize(RadioScope {
            source_key: stable_source_key("f1-25"),
            session_uid: Some(42),
            timeline_revision: 0,
            state_revisions: revisions,
            safe_to_speak: true,
        });
        for kind in ["position", "front_gap", "behind_gap", "yellow_flag"] {
            let mut job = voice_job(kind, kind, 1);
            job.state_revision = 1;
            assert!(mailbox.enqueue(job));
        }
        assert_eq!(mailbox.pending.len(), 4);

        revisions.position += 1;
        revisions.front_gap += 1;
        revisions.behind_gap += 1;
        revisions.track_flag += 1;
        mailbox.synchronize(RadioScope {
            source_key: stable_source_key("f1-25"),
            session_uid: Some(42),
            timeline_revision: 0,
            state_revisions: revisions,
            safe_to_speak: true,
        });
        assert!(mailbox.pending.is_empty());

        let mut program = voice_job("practice_program", "프로그램", 2);
        program.timeline_revision = 0;
        assert!(mailbox.enqueue(program));
        mailbox.synchronize(RadioScope {
            source_key: stable_source_key("f1-25"),
            session_uid: Some(42),
            timeline_revision: 1,
            state_revisions: revisions,
            safe_to_speak: true,
        });
        assert!(mailbox.pending.is_empty());

        let mut new_timeline = voice_job("practice_program", "새 타임라인", 3);
        new_timeline.timeline_revision = 1;
        assert!(mailbox.enqueue(new_timeline));
        mailbox.synchronize(RadioScope {
            source_key: stable_source_key("f1-25"),
            session_uid: Some(43),
            timeline_revision: 0,
            state_revisions: RadioStateRevisions::default(),
            safe_to_speak: true,
        });
        assert!(mailbox.pending.is_empty());

        let mut same_uid = voice_job("practice_program", "이전 소스", 4);
        same_uid.session_uid = Some(43);
        assert!(mailbox.enqueue(same_uid));
        mailbox.synchronize(RadioScope {
            source_key: stable_source_key("lmu"),
            session_uid: Some(43),
            timeline_revision: 0,
            state_revisions: RadioStateRevisions::default(),
            safe_to_speak: true,
        });
        assert!(mailbox.pending.is_empty());
    }

    #[test]
    fn voice_queue_drops_late_transient_calls_but_keeps_program_calls() {
        assert!(voice_job_is_stale(
            &voice_job("position", "현재 P2.", 1_000),
            7_000
        ));
        assert!(!voice_job_is_stale(
            &voice_job("practice_program", "다음 프로그램.", 1_000),
            7_000
        ));
        let mut safety_car = voice_job("safety_car", "세이프티 카.", 1_000);
        safety_car.priority = CallPriority::Critical;
        assert!(!voice_job_is_stale(&safety_car, 30_000));
        assert!(voice_job_is_stale(&safety_car, 62_000));
    }

    #[test]
    fn voice_queue_suppresses_recent_duplicate_and_gap_chatter() {
        let mut last_spoken = HashMap::new();
        let first_job = voice_job("front_gap", "앞차 0.8초.", 10_000);
        last_spoken.insert(
            voice_spoken_key(&first_job),
            LastSpokenRadio {
                spoken_at_unix_ms: 10_000,
                message: "앞차 0.8초.".to_owned(),
            },
        );

        assert!(should_suppress_voice_job(
            &voice_job("front_gap", "앞차 0.8초.", 11_000),
            11_000,
            &last_spoken
        ));
        assert!(should_suppress_voice_job(
            &voice_job("front_gap", "앞차 0.6초.", 11_000),
            11_000,
            &last_spoken
        ));
        assert!(!should_suppress_voice_job(
            &voice_job("front_gap", "앞차 0.6초.", 31_000),
            31_000,
            &last_spoken
        ));
    }

    #[test]
    fn routine_gap_updates_are_state_only_and_never_spoken() {
        assert!(call_is_voice_suppressed("front_gap", false));
        assert!(call_is_voice_suppressed("behind_gap", false));
        assert!(!call_is_voice_suppressed("safety_car", false));
        assert!(!call_is_voice_suppressed("fuel_target", false));
        assert!(call_is_voice_suppressed("safety_car", true));
        assert!(call_is_voice_suppressed("fuel_target", true));
    }

    #[test]
    fn ai_wakes_for_decisions_but_not_gap_or_position_noise() {
        for kind in [
            "engineer_online",
            "timeline_reset",
            "position",
            "front_gap",
            "behind_gap",
            "race_strategy_snapshot",
        ] {
            assert!(!ai_wake_event(kind), "unexpected AI wake for {kind}");
        }
        for kind in [
            "lap_complete",
            "safety_car",
            "front_wing_damage",
            "fuel_target",
            "ers_target",
            "rival_pit_front",
        ] {
            assert!(ai_wake_event(kind), "missing AI wake for {kind}");
        }
    }

    #[test]
    fn ai_safety_revisions_advance_for_weather_and_pit_changes() {
        let mut core = EngineerCore::default();
        core.ingest(&TelemetryUpdate {
            input: Some(driving_input(1.0)),
            ..TelemetryUpdate::default()
        });

        let mut dry = race_session(57);
        dry.session_time = 2.0;
        dry.frame_identifier = 2;
        let mut limiter = practice_status(0);
        limiter.session_time = 2.0;
        limiter.frame_identifier = 2;
        core.ingest(&TelemetryUpdate {
            session: Some(dry.clone()),
            status: Some(limiter.clone()),
            ..TelemetryUpdate::default()
        });
        let baseline = core.radio_revisions;

        dry.session_time = 3.0;
        dry.frame_identifier = 3;
        dry.weather = 3;
        core.ingest(&TelemetryUpdate {
            session: Some(dry),
            ..TelemetryUpdate::default()
        });
        assert!(core.radio_revisions.conditions > baseline.conditions);

        limiter.session_time = 4.0;
        limiter.frame_identifier = 4;
        limiter.pit_limiter_active = true;
        core.ingest(&TelemetryUpdate {
            status: Some(limiter),
            ..TelemetryUpdate::default()
        });
        assert!(core.radio_revisions.pit > baseline.pit);
    }

    #[test]
    fn hook_snapshot_paths_are_sequence_unique() {
        let base = Path::new("live-engineer/trigger.json");
        let first = hook_snapshot_path(base, 1);
        let second = hook_snapshot_path(base, 2);
        assert_ne!(first, second);
        assert_eq!(first.parent(), base.parent());
        assert_eq!(
            first.file_name().and_then(|name| name.to_str()),
            Some(".trigger.json.hook-1.json")
        );
    }

    #[test]
    fn ai_task_id_is_injected_into_the_hook_environment() {
        let environment = HookEnvironment {
            name: "SIM_MOZA_ENGINEER_TASK_ID",
            value: "01234567-89ab-cdef-0123-456789abcdef".to_owned(),
        };
        let command = prepare_hook_command(
            Path::new("engineer.ps1"),
            Path::new("trigger.json"),
            "SIM_MOZA_ENGINEER_AI_TRIGGER",
            Some(&environment),
        );
        let task_id = command
            .get_envs()
            .find(|(name, _)| *name == std::ffi::OsStr::new("SIM_MOZA_ENGINEER_TASK_ID"))
            .and_then(|(_, value)| value)
            .and_then(std::ffi::OsStr::to_str);
        assert_eq!(task_id, Some(environment.value.as_str()));
    }
