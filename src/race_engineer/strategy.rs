const RACE_LAP_HISTORY_LIMIT: usize = 24;
const MIN_STINT_TREND_LAPS: usize = 6;
const TYRE_DEGRADATION_NOTICE_MS_PER_LAP: f32 = 120.0;
const TYRE_DEGRADATION_HIGH_MS_PER_LAP: f32 = 250.0;
const FUEL_DEFICIT_LAPS: f32 = -0.3;
const ERS_LOW_PERCENT: f32 = 10.0;
const ERS_RECOVERY_PERCENT: f32 = 20.0;

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PracticeLapRecord {
    lap_num: u8,
    lap_time_ms: u32,
    clean: bool,
    tyre_compound: Option<u8>,
    tyre_age_laps: Option<u8>,
    fuel_kg: Option<f32>,
    setup_signature: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct PracticeProgram {
    phase: &'static str,
    objective: String,
    target_timed_laps: u8,
    completed_clean_laps: usize,
    current_setup_clean_laps: usize,
    instructions: Vec<String>,
    basis: Vec<String>,
    setup_candidate: Option<SetupRecommendation>,
    recent_laps: Vec<PracticeLapRecord>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct PracticeAdvisor {
    session_uid: Option<u64>,
    laps: Vec<PracticeLapRecord>,
    latest_setup_candidate: Option<SetupRecommendation>,
    last_announced_program: Option<ProgramAnnouncementKey>,
}

impl PracticeAdvisor {
    fn sync_session(&mut self, session_uid: Option<u64>, snapshot: &EngineerSnapshot) -> bool {
        let mut changed = false;
        if self.session_uid.is_some() && session_uid.is_some() && self.session_uid != session_uid {
            *self = Self::default();
            changed = true;
        }
        self.session_uid = session_uid;
        if snapshot
            .session
            .as_ref()
            .is_some_and(|session| !matches!(session.session_type, 1..=4))
        {
            changed |= !self.laps.is_empty() || self.latest_setup_candidate.is_some();
            self.laps.clear();
            self.latest_setup_candidate = None;
        }
        changed
    }

    fn observe(&mut self, lap: &CompletedLapAnalysis, snapshot: &EngineerSnapshot) {
        if !snapshot
            .session
            .as_ref()
            .is_some_and(|session| matches!(session.session_type, 1..=4))
        {
            return;
        }

        let baseline_setup = self
            .laps
            .iter()
            .find(|record| record.clean)
            .and_then(|record| record.setup_signature.as_ref());
        let current_setup = snapshot.setup.as_ref().map(setup_signature);
        let setup_changed = baseline_setup
            .zip(current_setup.as_ref())
            .is_some_and(|(baseline, current)| baseline != current);
        if lap.clean && !setup_changed {
            self.latest_setup_candidate = lap
                .recommendations
                .iter()
                .find(|recommendation| recommendation.area != "Baseline validation")
                .cloned();
        }
        self.laps.push(PracticeLapRecord {
            lap_num: lap.lap_num,
            lap_time_ms: lap.lap_time_ms,
            clean: lap.clean,
            tyre_compound: snapshot
                .status
                .as_ref()
                .map(|status| status.visual_tyre_compound),
            tyre_age_laps: snapshot.status.as_ref().map(|status| status.tyres_age_laps),
            fuel_kg: snapshot.status.as_ref().map(|status| status.fuel_in_tank),
            setup_signature: snapshot.setup.as_ref().map(setup_signature),
        });
        if self.laps.len() > 12 {
            self.laps.remove(0);
        }
    }

    fn plan(&self, snapshot: &EngineerSnapshot) -> Option<PracticeProgram> {
        let session = snapshot
            .session
            .as_ref()
            .filter(|session| matches!(session.session_type, 1..=4))?;
        let status = snapshot.status.as_ref();
        let clean_laps = self.laps.iter().filter(|lap| lap.clean).count();
        let current_setup_signature = snapshot.setup.as_ref().map(setup_signature);
        let current_setup_clean_laps = current_setup_signature.as_ref().map_or(0, |signature| {
            self.laps
                .iter()
                .filter(|lap| lap.clean && lap.setup_signature.as_ref() == Some(signature))
                .count()
        });
        let setup_changed = self
            .laps
            .iter()
            .find(|lap| lap.clean)
            .and_then(|lap| lap.setup_signature.as_ref())
            .zip(current_setup_signature.as_ref())
            .is_some_and(|(baseline, current)| baseline != current);
        let in_garage = snapshot
            .lap
            .as_ref()
            .is_some_and(|lap| lap.driver_status == 0 && lap.pit_status != 0);
        let practice_tyre_set = snapshot
            .tyre_sets
            .as_ref()
            .and_then(select_practice_tyre_set);
        let fitted_tyre_set = snapshot
            .tyre_sets
            .as_ref()
            .and_then(|tyre_sets| tyre_sets.sets.iter().find(|set| set.fitted));

        let mut basis = vec![format!("세션 잔여 {}분", session.session_time_left_s / 60)];
        if let Some(status) = status {
            basis.push(format!(
                "현재 컴파운드 코드 {}, 타이어 {}랩 사용, 연료 {:.1}kg",
                status.visual_tyre_compound, status.tyres_age_laps, status.fuel_in_tank
            ));
        }
        basis.push(format!("수집된 클린 랩 {}개", clean_laps));
        if let Some(tyre_sets) = &snapshot.tyre_sets {
            let available = tyre_sets
                .sets
                .iter()
                .filter(|set| set.available && !set.fitted)
                .count();
            basis.push(format!("교체 가능한 타이어 세트 {}개", available));
        }

        let used_tyre_stint = status.is_some_and(|status| status.tyres_age_laps >= 3);
        let (phase, objective, target_timed_laps, instructions, setup_candidate) =
            if setup_changed && current_setup_clean_laps < 2 {
                (
                    "setup_validation",
                    "변경 세팅 A/B 검증".to_owned(),
                    (2 - current_setup_clean_laps) as u8,
                    vec![
                        "연료 모드와 ERS 배치를 베이스라인 랩과 동일하게 유지".to_owned(),
                        "클린 랩 두 개를 확보한 뒤 랩타임과 코너별 입력을 함께 비교".to_owned(),
                    ],
                    self.latest_setup_candidate.clone(),
                )
            } else if session.session_type == 2 && clean_laps < 5 {
                let mut instructions = vec![
                    "레이스 연료와 레이스 ERS 모드로 일정한 페이스 유지".to_owned(),
                    "첫 랩부터 밀지 말고 5랩 동안 랩타임과 타이어 열화 확인".to_owned(),
                ];
                let p2_medium = fitted_tyre_set
                    .filter(|set| set.visual_tyre_compound == 17)
                    .or_else(|| practice_tyre_set.filter(|set| set.visual_tyre_compound == 17));
                if in_garage && let Some(tyre_set) = p2_medium {
                    instructions.insert(
                        0,
                        format!(
                            "{} 세트 {}번 {}: 마모 {}%, 예상 수명 {}랩",
                            tyre_compound_name(tyre_set.visual_tyre_compound),
                            tyre_set.index,
                            if tyre_set.fitted { "유지" } else { "장착" },
                            tyre_set.wear_percent,
                            tyre_set.life_span_laps
                        ),
                    );
                }
                (
                    "race_stint",
                    "P2 미디엄 레이스 시뮬레이션".to_owned(),
                    (5 - clean_laps) as u8,
                    instructions,
                    None,
                )
            } else if in_garage
                && clean_laps < 2
                && let Some(tyre_set) = practice_tyre_set
            {
                (
                    "baseline",
                    "레이스 컴파운드 베이스라인 확보".to_owned(),
                    2,
                    vec![
                        format!(
                            "{} 세트 {}번 장착: 마모 {}%, 예상 수명 {}랩",
                            tyre_compound_name(tyre_set.visual_tyre_compound),
                            tyre_set.index,
                            tyre_set.wear_percent,
                            tyre_set.life_span_laps
                        ),
                        "현재 세팅과 ERS 모드를 유지해 클린 타임드 랩 두 개 확보".to_owned(),
                    ],
                    None,
                )
            } else if used_tyre_stint && clean_laps < 3 {
                (
                    "race_stint",
                    "현재 타이어 롱런 추세 확보".to_owned(),
                    (3 - clean_laps) as u8,
                    vec![
                        "현재 세팅과 ERS 모드를 유지해 연속 랩 표본 확보".to_owned(),
                        "타이어를 과열시키지 말고 레이스 페이스로 일정하게 주행".to_owned(),
                    ],
                    None,
                )
            } else if clean_laps < 2 {
                (
                    "baseline",
                    "동일 조건 베이스라인 확보".to_owned(),
                    (2 - clean_laps) as u8,
                    vec![
                        "세팅을 바꾸지 말고 같은 컴파운드와 ERS 모드로 주행".to_owned(),
                        "클린 타임드 랩 두 개를 확보".to_owned(),
                    ],
                    None,
                )
            } else if setup_changed {
                (
                    "setup_review",
                    "A/B 결과 리뷰".to_owned(),
                    0,
                    vec!["피트로 복귀. 추가 변경 없이 비교 결과 확인".to_owned()],
                    None,
                )
            } else if let Some(candidate) = self.latest_setup_candidate.clone() {
                (
                    "setup_test",
                    "단일 세팅 변경 검증".to_owned(),
                    2,
                    vec![
                        candidate.action.clone(),
                        "한 번에 한 항목만 바꾸고 나머지 조건을 유지".to_owned(),
                    ],
                    Some(candidate),
                )
            } else if session.session_time_left_s >= 900 {
                (
                    "race_stint",
                    "레이스 페이스와 타이어 열화 확인".to_owned(),
                    5,
                    vec![
                        "레이스 연료와 레이스 ERS 모드로 연속 주행".to_owned(),
                        "첫 랩부터 밀지 말고 랩타임 편차와 마모 증가를 확인".to_owned(),
                    ],
                    None,
                )
            } else {
                (
                    "qualifying_simulation",
                    "저연료 퀄리파잉 시뮬레이션".to_owned(),
                    2,
                    vec![
                        "아웃랩에서 타이어를 준비하고 클린 푸시 랩 확보".to_owned(),
                        "두 번째 시도 전 회복 랩으로 ERS와 타이어 온도 정리".to_owned(),
                    ],
                    None,
                )
            };

        Some(PracticeProgram {
            phase,
            objective,
            target_timed_laps,
            completed_clean_laps: clean_laps,
            current_setup_clean_laps,
            instructions,
            basis,
            setup_candidate,
            recent_laps: self.laps.iter().rev().take(6).cloned().collect(),
        })
    }
}

#[derive(Clone, Debug, Serialize)]
struct RaceLapRecord {
    lap_num: u8,
    lap_time_ms: u32,
    clean: bool,
    position: u8,
    tyre_compound: Option<u8>,
    tyre_age_laps: Option<u8>,
    tyre_wear: Option<WheelValuesF32>,
    tyre_damage_excess_percent: Option<f32>,
    max_tyre_blister_percent: Option<u8>,
    fuel_delta_laps: Option<f32>,
    ers_percent: Option<f32>,
    front_gap_ms: Option<u32>,
    behind_gap_ms: Option<u32>,
    pit_stops: u8,
    safety_car_status: u8,
}

#[derive(Clone, Debug, Default, Serialize)]
struct RaceStrategySummary {
    current_stint_start_lap: Option<u8>,
    representative_stint_laps: usize,
    pace_trend_s_per_lap: Option<f32>,
    limiting_tyre: Option<&'static str>,
    max_tyre_wear_percent: Option<f32>,
    projected_finish_wear_percent: Option<f32>,
    fuel_delta_laps: Option<f32>,
    ers_percent: Option<f32>,
    pit_window_ideal_lap: Option<u8>,
    pit_window_latest_lap: Option<u8>,
    predicted_rejoin_position: Option<u8>,
    traffic_window: Vec<RaceTrafficCar>,
    recent_laps: Vec<RaceLapRecord>,
}

#[derive(Clone, Debug, Serialize)]
struct RaceTrafficCar {
    car_index: u8,
    position: u8,
    current_lap_num: u8,
    delta_to_car_in_front_ms: Option<u32>,
    delta_to_race_leader_ms: Option<u32>,
    pit_status: u8,
    num_pit_stops: u8,
    driver_status: u8,
    result_status: u8,
}

#[derive(Clone, Copy, Debug, Default)]
struct StintMetrics {
    representative_laps: usize,
    pace_trend_ms_per_lap: Option<f32>,
    max_wear: Option<(f32, &'static str)>,
    projected_finish_wear: Option<f32>,
}

#[derive(Default)]
struct RaceStrategyAdvisor {
    session_uid: Option<u64>,
    laps: Vec<RaceLapRecord>,
    current_stint_start_lap: Option<u8>,
    used_dry_compounds: u8,
    wet_compound_seen: bool,
    degradation_level: u8,
    fuel_deficit_streak: u8,
    fuel_warned: bool,
    ers_low_streak: u8,
    ers_recovery_streak: u8,
    ers_warned: bool,
    pit_window_announced: bool,
    pit_window_latest_announced: bool,
    endgame_announced: bool,
}

impl RaceStrategyAdvisor {
    fn sync_session(&mut self, session_uid: Option<u64>, snapshot: &EngineerSnapshot) {
        if self.session_uid.is_some() && session_uid.is_some() && self.session_uid != session_uid {
            *self = Self::default();
        }
        self.session_uid = session_uid;
        if snapshot
            .session
            .as_ref()
            .is_some_and(|session| !matches!(session.session_type, 15..=17))
        {
            self.laps.clear();
            self.current_stint_start_lap = None;
        }
    }

    fn rewind_to_lap(&mut self, current_lap: u8) {
        if !self.laps.iter().any(|lap| lap.lap_num >= current_lap) {
            return;
        }
        self.laps.retain(|lap| lap.lap_num < current_lap);
        self.rebuild_stint_start();
        self.rebuild_observed_compounds();
        self.degradation_level = 0;
        self.fuel_deficit_streak = 0;
        self.fuel_warned = false;
        self.ers_low_streak = 0;
        self.ers_recovery_streak = 0;
        self.ers_warned = false;
        self.pit_window_announced = false;
        self.pit_window_latest_announced = false;
        self.endgame_announced = false;
    }

    fn observe(
        &mut self,
        completed_lap: &CompletedLapAnalysis,
        snapshot: &EngineerSnapshot,
    ) -> Vec<EngineerCall> {
        let Some(session) = snapshot
            .session
            .as_ref()
            .filter(|session| matches!(session.session_type, 15..=17))
        else {
            return Vec::new();
        };
        let Some(lap) = snapshot.lap.as_ref() else {
            return Vec::new();
        };

        self.rewind_to_lap(completed_lap.lap_num);
        let status = completed_lap.latest_status.as_ref();
        let damage = completed_lap.latest_damage.as_ref();
        let record = RaceLapRecord {
            lap_num: completed_lap.lap_num,
            lap_time_ms: completed_lap.lap_time_ms,
            clean: completed_lap.clean,
            position: lap.car_position,
            tyre_compound: status.map(|status| status.visual_tyre_compound),
            tyre_age_laps: status.map(|status| status.tyres_age_laps),
            tyre_wear: damage.map(|damage| damage.tyre_wear),
            tyre_damage_excess_percent: damage.map(max_excess_tyre_damage),
            max_tyre_blister_percent: damage.map(max_tyre_blister),
            fuel_delta_laps: status.and_then(|status| status.fuel_delta_laps),
            ers_percent: status.map(StatusSample::ers_percent),
            front_gap_ms: lap.delta_to_car_in_front_ms,
            behind_gap_ms: lap.delta_to_car_behind_ms,
            pit_stops: lap.num_pit_stops,
            safety_car_status: session.safety_car_status,
        };
        let new_stint = self.laps.last().is_none_or(|previous| {
            previous.tyre_compound != record.tyre_compound
                || previous.pit_stops != record.pit_stops
                || previous
                    .tyre_age_laps
                    .zip(record.tyre_age_laps)
                    .is_some_and(|(previous, current)| current < previous)
        });
        if new_stint {
            self.current_stint_start_lap = Some(record.lap_num);
            self.degradation_level = 0;
            self.pit_window_announced = false;
            self.pit_window_latest_announced = false;
            self.endgame_announced = false;
        }
        if let Some(compound) = record.tyre_compound {
            if let Some(bit) = dry_compound_bit(compound) {
                self.used_dry_compounds |= bit;
            } else if matches!(compound, 7 | 8) {
                self.wet_compound_seen = true;
            }
        }
        self.update_resource_streaks(&record);
        self.laps.push(record);
        if self.laps.len() > RACE_LAP_HISTORY_LIMIT {
            self.laps.remove(0);
        }

        if !completed_lap.clean || completed_lap.lap_num >= session.total_laps {
            return Vec::new();
        }

        let metrics = self.stint_metrics(session.total_laps);
        let action = (session.safety_car_status == 0).then(|| {
            self.endgame_call(session, metrics)
                .or_else(|| self.fuel_call())
                .or_else(|| self.pit_window_call(session))
                .or_else(|| self.degradation_call(metrics))
                .or_else(|| self.ers_call())
        });

        let mut calls = action.into_iter().flatten().collect::<Vec<_>>();
        if completed_lap.lap_num.is_multiple_of(10) {
            calls.push(self.strategy_snapshot_call(session, metrics));
        }
        calls
    }

    fn update_resource_streaks(&mut self, record: &RaceLapRecord) {
        if record.clean
            && record.safety_car_status == 0
            && record
                .fuel_delta_laps
                .is_some_and(|delta| delta < FUEL_DEFICIT_LAPS)
        {
            self.fuel_deficit_streak = self.fuel_deficit_streak.saturating_add(1);
        } else {
            self.fuel_deficit_streak = 0;
            if record.fuel_delta_laps.is_some_and(|delta| delta >= 0.0) {
                self.fuel_warned = false;
            }
        }

        if record.clean
            && record.safety_car_status == 0
            && record.ers_percent.is_some_and(|ers| ers < ERS_LOW_PERCENT)
        {
            self.ers_low_streak = self.ers_low_streak.saturating_add(1);
            self.ers_recovery_streak = 0;
        } else if record
            .ers_percent
            .is_some_and(|ers| ers >= ERS_RECOVERY_PERCENT)
        {
            self.ers_low_streak = 0;
            self.ers_recovery_streak = self.ers_recovery_streak.saturating_add(1);
            if self.ers_recovery_streak >= 2 {
                self.ers_warned = false;
            }
        } else {
            self.ers_low_streak = 0;
            self.ers_recovery_streak = 0;
        }
    }

    fn endgame_call(
        &mut self,
        session: &SessionSample,
        metrics: StintMetrics,
    ) -> Option<EngineerCall> {
        let last = self.laps.last()?;
        let remaining_laps = session.total_laps.saturating_sub(last.lap_num);
        let projected_wear = metrics.projected_finish_wear?;
        let conditions_allow_stay_out = self.stay_out_conditions(
            session,
            last,
            metrics,
            last.fuel_delta_laps,
            last.tyre_damage_excess_percent,
            last.max_tyre_blister_percent,
        );
        if self.endgame_announced {
            if !conditions_allow_stay_out && (1..=3).contains(&remaining_laps) {
                self.endgame_announced = false;
                return Some(Self::strategy_reassess_call());
            }
            return None;
        }
        if !conditions_allow_stay_out {
            return None;
        }
        self.endgame_announced = true;
        Some(EngineerCall::important(
            "strategy_stay_out",
            format!(
                "남은 {remaining_laps}랩. 완주 예상 타이어 마모 {projected_wear:.0}%. 이상 없으면 스테이 아웃, 트랙 포지션을 지킨다."
            ),
        ))
    }

    fn stay_out_conditions(
        &self,
        session: &SessionSample,
        last: &RaceLapRecord,
        metrics: StintMetrics,
        fuel_delta_laps: Option<f32>,
        tyre_damage_excess_percent: Option<f32>,
        max_tyre_blister_percent: Option<u8>,
    ) -> bool {
        let remaining_laps = session.total_laps.saturating_sub(last.lap_num);
        let imminent_rain = session.weather_forecast_samples.iter().any(|forecast| {
            forecast.time_offset_min <= 10
                && (forecast.session_type == 0 || forecast.session_type == session.session_type)
                && forecast.rain_percentage >= 40
        });
        let tyre_integrity_ok = tyre_damage_excess_percent
            .zip(max_tyre_blister_percent)
            .is_some_and(|(damage, blister)| damage < 10.0 && blister < 10);
        let compound_obligation_satisfied =
            self.wet_compound_seen || self.used_dry_compounds.count_ones() >= 2;
        let pit_would_lose_track_position = session
            .pit_stop_rejoin_position
            .is_some_and(|rejoin| rejoin > last.position);
        (1..=3).contains(&remaining_laps)
            && session.safety_car_status == 0
            && session.weather < 3
            && !imminent_rain
            && metrics
                .projected_finish_wear
                .is_some_and(|wear| wear < 70.0)
            && fuel_delta_laps.is_some_and(|delta| delta >= 0.0)
            && tyre_integrity_ok
            && compound_obligation_satisfied
            && pit_would_lose_track_position
    }

    fn reassess_live_conditions(&mut self, snapshot: &EngineerSnapshot) -> Option<EngineerCall> {
        if !self.endgame_announced {
            return None;
        }
        let session = snapshot
            .session
            .as_ref()
            .filter(|session| matches!(session.session_type, 15..=17))?;
        let last = self.laps.last()?;
        let remaining_laps = session.total_laps.saturating_sub(last.lap_num);
        if !(1..=3).contains(&remaining_laps) {
            return None;
        }
        let metrics = self.stint_metrics(session.total_laps);
        let fuel_delta = snapshot
            .status
            .as_ref()
            .and_then(|status| status.fuel_delta_laps)
            .or(last.fuel_delta_laps);
        let damage = snapshot.damage.as_ref();
        let tyre_damage = damage
            .map(max_excess_tyre_damage)
            .or(last.tyre_damage_excess_percent);
        let blister = damage
            .map(max_tyre_blister)
            .or(last.max_tyre_blister_percent);
        if self.stay_out_conditions(session, last, metrics, fuel_delta, tyre_damage, blister) {
            return None;
        }
        self.endgame_announced = false;
        Some(Self::strategy_reassess_call())
    }

    fn strategy_reassess_call() -> EngineerCall {
        EngineerCall::important(
            "strategy_reassess",
            "조건 변경. 스테이 아웃 계획 취소, 타이어·날씨·연료와 의무 컴파운드를 다시 확인한다.",
        )
    }

    fn fuel_call(&mut self) -> Option<EngineerCall> {
        let last = self.laps.last()?;
        let delta = last.fuel_delta_laps?;
        if self.fuel_warned || self.fuel_deficit_streak < 2 {
            return None;
        }
        self.fuel_warned = true;
        Some(EngineerCall::important(
            "fuel_target",
            format!("연료 델타 {delta:+.1}랩. 다음 2랩 리프트 앤 코스트, 델타 0.0 회복이 목표다."),
        ))
    }

    fn pit_window_call(&mut self, session: &SessionSample) -> Option<EngineerCall> {
        if self.endgame_announced {
            return None;
        }
        let last = self.laps.last()?;
        let next_lap = last.lap_num.saturating_add(1);
        let ideal = session.pit_stop_window_ideal_lap?;
        let latest = session.pit_stop_window_latest_lap.unwrap_or(ideal);
        if next_lap < ideal {
            return None;
        }

        let rejoin = session
            .pit_stop_rejoin_position
            .map(|position| format!(", 게임 예상 재합류 P{position}"))
            .unwrap_or_default();
        if next_lap >= latest && !self.pit_window_latest_announced {
            self.pit_window_announced = true;
            self.pit_window_latest_announced = true;
            return Some(EngineerCall::important(
                "pit_window_latest",
                format!(
                    "전략 피트 윈도 마지막 랩 {latest}{rejoin}. 타이어와 교통 확인 후 결정한다."
                ),
            ));
        }
        if !self.pit_window_announced {
            self.pit_window_announced = true;
            return Some(EngineerCall::important(
                "pit_window_open",
                format!(
                    "전략 피트 윈도 {ideal}에서 {latest}랩{rejoin}. 지금은 타이어 추세와 트랙 포지션을 함께 본다."
                ),
            ));
        }
        None
    }

    fn degradation_call(&mut self, metrics: StintMetrics) -> Option<EngineerCall> {
        if metrics.representative_laps < MIN_STINT_TREND_LAPS {
            return None;
        }
        let trend = metrics.pace_trend_ms_per_lap?;
        let level = if trend >= TYRE_DEGRADATION_HIGH_MS_PER_LAP {
            2
        } else if trend >= TYRE_DEGRADATION_NOTICE_MS_PER_LAP {
            1
        } else {
            0
        };
        if level <= self.degradation_level {
            return None;
        }
        self.degradation_level = level;
        let (wear, tyre) = metrics.max_wear.unwrap_or((0.0, "타이어"));
        Some(EngineerCall::important(
            "tyre_degradation",
            format!(
                "타이어 열화 추세 랩당 +{:.2}초. 제한은 {tyre} {wear:.0}%. 2랩 관리하고 피트 윈도를 다시 본다.",
                trend / 1_000.0
            ),
        ))
    }

    fn ers_call(&mut self) -> Option<EngineerCall> {
        let last = self.laps.last()?;
        let ers = last.ers_percent?;
        if self.ers_warned || self.ers_low_streak < 3 {
            return None;
        }
        self.ers_warned = true;
        let combat = last.front_gap_ms.is_some_and(|gap| gap <= 1_500)
            || last.behind_gap_ms.is_some_and(|gap| gap <= 1_500);
        let action = if combat {
            "이번 랩은 방어·공격 지점 외 배치를 줄이고"
        } else {
            "이번 랩 배치를 줄이고"
        };
        Some(EngineerCall::important(
            "ers_target",
            format!("ERS {ers:.0}%. {action} 다음 전투 전 20% 회복이 목표다."),
        ))
    }

    fn strategy_snapshot_call(
        &self,
        session: &SessionSample,
        metrics: StintMetrics,
    ) -> EngineerCall {
        let last = self.laps.last().expect("a completed lap was just recorded");
        let trend = metrics
            .pace_trend_ms_per_lap
            .map(|trend| format!("페이스 추세 {:+.2}초/랩", trend / 1_000.0))
            .unwrap_or_else(|| "페이스 추세 표본 부족".to_owned());
        let wear = metrics
            .max_wear
            .map(|(wear, tyre)| format!("{tyre} 마모 {wear:.0}%"))
            .unwrap_or_else(|| "마모 데이터 없음".to_owned());
        EngineerCall::normal(
            "race_strategy_snapshot",
            format!(
                "L{}/{} P{}. {trend}, {wear}, 연료 {}, ERS {}.",
                last.lap_num,
                session.total_laps,
                last.position,
                last.fuel_delta_laps
                    .map(|delta| format!("{delta:+.1}랩"))
                    .unwrap_or_else(|| "없음".to_owned()),
                last.ers_percent
                    .map(|ers| format!("{ers:.0}%"))
                    .unwrap_or_else(|| "없음".to_owned())
            ),
        )
    }

    fn summary(&self, snapshot: &EngineerSnapshot) -> Option<RaceStrategySummary> {
        let session = snapshot
            .session
            .as_ref()
            .filter(|session| matches!(session.session_type, 15..=17))?;
        let metrics = self.stint_metrics(session.total_laps);
        let last = self.laps.last();
        Some(RaceStrategySummary {
            current_stint_start_lap: self.current_stint_start_lap,
            representative_stint_laps: metrics.representative_laps,
            pace_trend_s_per_lap: metrics.pace_trend_ms_per_lap.map(|trend| trend / 1_000.0),
            limiting_tyre: metrics.max_wear.map(|(_, tyre)| tyre),
            max_tyre_wear_percent: metrics.max_wear.map(|(wear, _)| wear),
            projected_finish_wear_percent: metrics.projected_finish_wear,
            fuel_delta_laps: last.and_then(|lap| lap.fuel_delta_laps),
            ers_percent: last.and_then(|lap| lap.ers_percent),
            pit_window_ideal_lap: session.pit_stop_window_ideal_lap,
            pit_window_latest_lap: session.pit_stop_window_latest_lap,
            predicted_rejoin_position: session.pit_stop_rejoin_position,
            traffic_window: race_traffic_window(snapshot),
            recent_laps: self.laps.iter().rev().take(8).cloned().collect(),
        })
    }

    fn stint_metrics(&self, total_laps: u8) -> StintMetrics {
        let Some(start_lap) = self.current_stint_start_lap else {
            return StintMetrics::default();
        };
        let mut representative = self
            .laps
            .iter()
            .filter(|lap| {
                lap.lap_num >= start_lap
                    && lap.lap_num > 1
                    && lap.clean
                    && lap.safety_car_status == 0
                    && (10_000..600_000).contains(&lap.lap_time_ms)
            })
            .collect::<Vec<_>>();
        if let Some(median) = median_f32(
            representative
                .iter()
                .map(|lap| lap.lap_time_ms as f32)
                .collect(),
        ) {
            representative.retain(|lap| lap.lap_time_ms as f32 <= median + 1_500.0);
        }

        let pace_trend_ms_per_lap = if representative.len() >= MIN_STINT_TREND_LAPS {
            theil_sen_lap_slope(&representative)
        } else {
            None
        };
        let last = self.laps.last();
        let max_wear = last.and_then(|lap| lap.tyre_wear).map(max_wheel);
        let projected_finish_wear = last.and_then(|last| {
            let current_wear = max_wheel(last.tyre_wear?).0;
            let first = self.laps.iter().find(|lap| {
                lap.lap_num >= start_lap && lap.tyre_wear.is_some() && lap.lap_num < last.lap_num
            })?;
            let first_wear = max_wheel(first.tyre_wear?).0;
            let lap_span = last.lap_num.saturating_sub(first.lap_num) as f32;
            if lap_span <= 0.0 {
                return Some(current_wear);
            }
            let wear_rate = ((current_wear - first_wear) / lap_span).max(0.0);
            let remaining = total_laps.saturating_sub(last.lap_num) as f32;
            Some((current_wear + wear_rate * remaining).clamp(0.0, 100.0))
        });
        StintMetrics {
            representative_laps: representative.len(),
            pace_trend_ms_per_lap,
            max_wear,
            projected_finish_wear,
        }
    }

    fn rebuild_stint_start(&mut self) {
        let Some(last) = self.laps.last() else {
            self.current_stint_start_lap = None;
            return;
        };
        let mut start = last.lap_num;
        let mut next = last;
        for previous in self.laps.iter().rev().skip(1) {
            let same_stint = previous.tyre_compound == next.tyre_compound
                && previous.pit_stops == next.pit_stops
                && previous
                    .tyre_age_laps
                    .zip(next.tyre_age_laps)
                    .is_none_or(|(previous, current)| previous <= current);
            if !same_stint {
                break;
            }
            start = previous.lap_num;
            next = previous;
        }
        self.current_stint_start_lap = Some(start);
    }

    fn rebuild_observed_compounds(&mut self) {
        self.used_dry_compounds = 0;
        self.wet_compound_seen = false;
        for lap in &self.laps {
            if let Some(compound) = lap.tyre_compound {
                if let Some(bit) = dry_compound_bit(compound) {
                    self.used_dry_compounds |= bit;
                } else if matches!(compound, 7 | 8) {
                    self.wet_compound_seen = true;
                }
            }
        }
    }
}

fn dry_compound_bit(compound: u8) -> Option<u8> {
    match compound {
        16 => Some(1),
        17 => Some(2),
        18 => Some(4),
        _ => None,
    }
}

fn race_traffic_window(snapshot: &EngineerSnapshot) -> Vec<RaceTrafficCar> {
    let (Some(order), Some(lap)) = (snapshot.race_order.as_ref(), snapshot.lap.as_ref()) else {
        return Vec::new();
    };
    let player_position = lap.car_position;
    let rejoin_position = snapshot
        .session
        .as_ref()
        .and_then(|session| session.pit_stop_rejoin_position);
    let mut cars = order
        .cars
        .iter()
        .filter(|car| {
            car.car_position > 0
                && (car.car_position.abs_diff(player_position) <= 2
                    || rejoin_position
                        .is_some_and(|position| car.car_position.abs_diff(position) <= 2))
        })
        .map(|car| RaceTrafficCar {
            car_index: car.car_index,
            position: car.car_position,
            current_lap_num: car.current_lap_num,
            delta_to_car_in_front_ms: car.delta_to_car_in_front_ms,
            delta_to_race_leader_ms: car.delta_to_race_leader_ms,
            pit_status: car.pit_status,
            num_pit_stops: car.num_pit_stops,
            driver_status: car.driver_status,
            result_status: car.result_status,
        })
        .collect::<Vec<_>>();
    cars.sort_by_key(|car| car.position);
    cars.truncate(10);
    cars
}

fn median_f32(mut values: Vec<f32>) -> Option<f32> {
    values.retain(|value| value.is_finite());
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        Some((values[middle - 1] + values[middle]) / 2.0)
    } else {
        Some(values[middle])
    }
}

fn theil_sen_lap_slope(laps: &[&RaceLapRecord]) -> Option<f32> {
    let mut slopes = Vec::new();
    for (index, left) in laps.iter().enumerate() {
        for right in laps.iter().skip(index + 1) {
            let lap_delta = right.lap_num.saturating_sub(left.lap_num);
            if lap_delta > 0 {
                slopes
                    .push((right.lap_time_ms as f32 - left.lap_time_ms as f32) / lap_delta as f32);
            }
        }
    }
    median_f32(slopes)
}

fn select_practice_tyre_set(tyre_sets: &TyreSetsSample) -> Option<&TyreSetInfo> {
    tyre_sets
        .sets
        .iter()
        .filter(|set| set.available && !set.fitted && matches!(set.visual_tyre_compound, 16..=18))
        .min_by_key(|set| {
            let compound_priority = match set.visual_tyre_compound {
                17 => 0_u16,
                18 => 1,
                16 => 2,
                _ => 3,
            };
            compound_priority * 101 + set.wear_percent as u16
        })
}

fn tyre_compound_name(visual_compound: u8) -> &'static str {
    match visual_compound {
        16 => "소프트",
        17 => "미디엄",
        18 => "하드",
        7 => "인터미디엇",
        8 => "웨트",
        _ => "타이어",
    }
}

fn setup_signature(setup: &CarSetupSample) -> String {
    format!(
        "{}:{}:{}:{}:{:.2}:{:.2}:{:.2}:{:.2}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{:.2}:{:.2}:{:.2}:{:.2}:{}",
        setup.front_wing,
        setup.rear_wing,
        setup.on_throttle_differential_percent,
        setup.off_throttle_differential_percent,
        setup.front_camber,
        setup.rear_camber,
        setup.front_toe,
        setup.rear_toe,
        setup.front_suspension,
        setup.rear_suspension,
        setup.front_anti_roll_bar,
        setup.rear_anti_roll_bar,
        setup.front_ride_height,
        setup.rear_ride_height,
        setup.brake_pressure_percent,
        setup.brake_bias_percent,
        setup.engine_braking_percent,
        setup.tyre_pressures_psi.fl,
        setup.tyre_pressures_psi.fr,
        setup.tyre_pressures_psi.rl,
        setup.tyre_pressures_psi.rr,
        setup.ballast
    )
}

#[derive(Serialize)]
struct ErsSummary {
    store_energy_j: f32,
    store_percent: f32,
    deploy_mode: u8,
    harvested_this_lap_mguk_j: f32,
    harvested_this_lap_mguh_j: f32,
    harvested_this_lap_total_j: f32,
    harvest_limit_per_lap_j: Option<f32>,
    deployed_this_lap_j: f32,
}

impl From<&StatusSample> for ErsSummary {
    fn from(value: &StatusSample) -> Self {
        Self {
            store_energy_j: value.ers_store_energy,
            store_percent: value.ers_percent(),
            deploy_mode: value.ers_deploy_mode,
            harvested_this_lap_mguk_j: value.ers_harvested_this_lap_mguk,
            harvested_this_lap_mguh_j: value.ers_harvested_this_lap_mguh,
            harvested_this_lap_total_j: value.ers_harvested_this_lap(),
            harvest_limit_per_lap_j: value.ers_harvest_limit_per_lap,
            deployed_this_lap_j: value.ers_deployed_this_lap,
        }
    }
}

#[derive(Serialize)]
struct EngineerTrigger<'a> {
    schema_version: u8,
    sequence: u64,
    timestamp_unix_ms: u128,
    timeline_revision: u64,
    timeline_reset: Option<&'a TimelineReset>,
    decision_mode: &'static str,
    reasons: Vec<&'static str>,
    calls: &'a [EngineerCall],
    completed_lap: Option<CompletedLapTrigger>,
    state_path: Option<&'a Path>,
    history_path: Option<&'a Path>,
    input_log_path: Option<&'a Path>,
    corner_log_path: Option<&'a Path>,
    analysis_report_path: Option<&'a Path>,
    radio_path: Option<&'a Path>,
    practice_state_path: Option<&'a Path>,
    state: LiveEngineerState<'a>,
}

#[derive(Serialize)]
struct CompletedLapTrigger {
    lap_num: u8,
    lap_time_ms: u32,
    clean: bool,
    invalid_reason: Option<String>,
    sample_count: usize,
    recommendations: Vec<SetupRecommendation>,
    setup: Option<CarSetupSample>,
}

impl From<&CompletedLapAnalysis> for CompletedLapTrigger {
    fn from(value: &CompletedLapAnalysis) -> Self {
        Self {
            lap_num: value.lap_num,
            lap_time_ms: value.lap_time_ms,
            clean: value.clean,
            invalid_reason: value.invalid_reason.clone(),
            sample_count: value.sample_count,
            recommendations: value.recommendations.clone(),
            setup: value.latest_setup.clone(),
        }
    }
}

fn session_type_name(value: u8) -> &'static str {
    match value {
        1..=4 => "practice",
        5..=9 => "qualifying",
        10..=14 => "sprint_qualifying",
        15..=17 => "race",
        18 => "time_trial",
        _ => "unknown",
    }
}
