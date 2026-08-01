const GAP_ALERT_MS: u32 = 1_000;
const GAP_CLEAR_MS: u32 = 1_500;
const GAP_CONFIRM_S: f32 = 1.0;
const GAP_CLEAR_CONFIRM_S: f32 = 1.0;
const FLASHBACK_THRESHOLD_S: f32 = 1.0;
const RIVAL_PIT_MAX_GAP_MS: u32 = 10_000;

#[derive(Clone, Copy, Debug, PartialEq)]
struct GapCandidate {
    since: f32,
    peer_index: Option<u8>,
    player_position: u8,
}

#[derive(Clone, Copy)]
struct GapObservation {
    gap_ms: Option<u32>,
    session_time: f32,
    peer_index: Option<u8>,
    player_position: u8,
}

struct GapTracker<'a> {
    is_close: &'a mut bool,
    candidate: &'a mut Option<GapCandidate>,
    clear_since: &'a mut Option<f32>,
    state_revision: &'a mut u64,
}

#[derive(Clone, Copy)]
struct GapRadioText {
    kind: &'static str,
    car: &'static str,
    context: &'static str,
}

#[derive(Default)]
struct EngineerSnapshot {
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

impl EngineerSnapshot {
    fn apply_filtered(
        &mut self,
        update: &TelemetryUpdate,
        lap_is_fresh: bool,
        race_order_is_fresh: bool,
        session_is_fresh: bool,
    ) {
        if update.packet_format.is_some() {
            self.packet_format = update.packet_format;
        }
        if update.session_uid.is_some() {
            self.session_uid = update.session_uid;
        }
        if let Some(input) = &update.input {
            self.input = Some(input.clone());
        }
        if lap_is_fresh && let Some(lap) = &update.lap {
            self.lap = Some(lap.clone());
        }
        if race_order_is_fresh && let Some(race_order) = &update.race_order {
            self.race_order = Some(race_order.clone());
        }
        if session_is_fresh && let Some(session) = &update.session {
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

    fn reset_timeline(&mut self) {
        let packet_format = self.packet_format;
        let session_uid = self.session_uid;
        *self = Self::default();
        self.packet_format = packet_format;
        self.session_uid = session_uid;
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
struct RadioStateRevisions {
    position: u64,
    front_gap: u64,
    behind_gap: u64,
    track_flag: u64,
    race_control: u64,
    conditions: u64,
    pit: u64,
    damage: u64,
    rival: u64,
    strategy: u64,
}

impl RadioStateRevisions {
    fn revision_for(self, key: &str) -> u64 {
        match key {
            "position" => self.position,
            "front_gap" => self.front_gap,
            "behind_gap" => self.behind_gap,
            "track_flag" => self.track_flag,
            "race_control" => self.race_control,
            "conditions" => self.conditions,
            "pit" => self.pit,
            "damage" => self.damage,
            "rival" => self.rival,
            "strategy" => self.strategy,
            _ => 0,
        }
    }

    fn invalidate_transient(&mut self) {
        self.position = self.position.saturating_add(1);
        self.front_gap = self.front_gap.saturating_add(1);
        self.behind_gap = self.behind_gap.saturating_add(1);
        self.track_flag = self.track_flag.saturating_add(1);
        self.race_control = self.race_control.saturating_add(1);
        self.conditions = self.conditions.saturating_add(1);
        self.pit = self.pit.saturating_add(1);
        self.rival = self.rival.saturating_add(1);
        self.strategy = self.strategy.saturating_add(1);
    }

    fn invalidate_all(&mut self) {
        self.invalidate_transient();
        self.damage = self.damage.saturating_add(1);
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct RadioScope {
    source_key: u64,
    session_uid: Option<u64>,
    timeline_revision: u64,
    state_revisions: RadioStateRevisions,
    safe_to_speak: bool,
}

#[derive(Debug, Default)]
struct EngineerCoreUpdate {
    calls: Vec<EngineerCall>,
    timeline_reset: Option<TimelineReset>,
}

#[derive(Default)]
struct EngineerCore {
    snapshot: EngineerSnapshot,
    driving: bool,
    last_motion_at: Option<Instant>,
    last_session_time: Option<f32>,
    lap_cursor: Option<SampleCursor>,
    race_order_cursor: Option<SampleCursor>,
    session_cursor: Option<SampleCursor>,
    timeline_revision: u64,
    last_timeline_reset: Option<TimelineReset>,
    online_announced: bool,
    radio_revisions: RadioStateRevisions,
    last_lap_num: Option<u8>,
    last_position: Option<u8>,
    lap_invalid: bool,
    front_gap_close: bool,
    behind_gap_close: bool,
    front_gap_candidate: Option<GapCandidate>,
    behind_gap_candidate: Option<GapCandidate>,
    front_gap_clear_since: Option<f32>,
    behind_gap_clear_since: Option<f32>,
    pit_limiter: Option<bool>,
    safety_car_status: Option<u8>,
    weather: Option<u8>,
    rain_risk_level: u8,
    nearby_pit_status: HashMap<u8, u8>,
    nearby_gap_ms: HashMap<u8, u32>,
    nearby_pit_announced: HashMap<u8, (u8, u8)>,
    last_front_peer: Option<u8>,
    last_behind_peer: Option<u8>,
    damage_initialized: bool,
    tyre_wear_level: u8,
    tyre_damage_level: u8,
    front_wing_level: u8,
    rear_wing_level: u8,
    gearbox_level: u8,
    engine_level: u8,
    current_flag: i8,
    final_announced: bool,
}

impl EngineerCore {
    #[cfg(test)]
    fn ingest(&mut self, update: &TelemetryUpdate) -> Vec<EngineerCall> {
        self.ingest_with_context(update).calls
    }

    fn ingest_with_context(&mut self, update: &TelemetryUpdate) -> EngineerCoreUpdate {
        let mut calls = Vec::new();
        if let Some(session_uid) = update.session_uid
            && self
                .snapshot
                .session_uid
                .is_some_and(|previous| previous != session_uid)
        {
            *self = Self::default();
        }
        let protected_samples = [
            update.lap.as_ref().map(|sample| {
                sample_is_stale(
                    self.lap_cursor,
                    sample.session_time,
                    sample.frame_identifier,
                    sample.overall_frame_identifier,
                )
            }),
            update.race_order.as_ref().map(|sample| {
                sample_is_stale(
                    self.race_order_cursor,
                    sample.session_time,
                    sample.frame_identifier,
                    sample.overall_frame_identifier,
                )
            }),
            update.session.as_ref().map(|sample| {
                sample_is_stale(
                    self.session_cursor,
                    sample.session_time,
                    sample.frame_identifier,
                    sample.overall_frame_identifier,
                )
            }),
        ];
        let has_protected_sample = protected_samples.iter().any(Option::is_some);
        let all_protected_samples_stale =
            has_protected_sample && protected_samples.into_iter().flatten().all(|stale| stale);
        let update_session_time = newest_session_time(update);
        let timeline_reset = if all_protected_samples_stale {
            None
        } else {
            self.last_session_time
                .zip(update_session_time)
                .filter(|(previous, current)| current + FLASHBACK_THRESHOLD_S < *previous)
                .map(|(previous, current)| self.reset_timeline(previous, current))
        };
        if let Some(reset) = timeline_reset {
            calls.push(EngineerCall::normal(
                "timeline_reset",
                format!(
                    "텔레메트리 타임라인 리셋. {:.1}초에서 {:.1}초로 되감기.",
                    reset.rollback_from_session_time, reset.rollback_to_session_time
                ),
            ));
        }
        if let Some(session_time) = update_session_time {
            self.last_session_time = Some(
                self.last_session_time
                    .map_or(session_time, |previous| previous.max(session_time)),
            );
        }

        let lap_is_fresh = update.lap.as_ref().is_none_or(|sample| {
            accept_sample(
                &mut self.lap_cursor,
                sample.session_time,
                sample.frame_identifier,
                sample.overall_frame_identifier,
            )
        });
        let race_order_is_fresh = update.race_order.as_ref().is_none_or(|sample| {
            accept_sample(
                &mut self.race_order_cursor,
                sample.session_time,
                sample.frame_identifier,
                sample.overall_frame_identifier,
            )
        });
        let session_is_fresh = update.session.as_ref().is_none_or(|sample| {
            accept_sample(
                &mut self.session_cursor,
                sample.session_time,
                sample.frame_identifier,
                sample.overall_frame_identifier,
            )
        });

        self.snapshot
            .apply_filtered(update, lap_is_fresh, race_order_is_fresh, session_is_fresh);

        if update.final_classification.is_some() {
            self.inspect_final_classification(&mut calls);
            return EngineerCoreUpdate {
                calls,
                timeline_reset,
            };
        }

        if let Some(input) = update.input.as_ref() {
            if is_driving(input) {
                self.last_motion_at = Some(Instant::now());
            } else if self.driving
                && self
                    .last_motion_at
                    .is_some_and(|last_motion| last_motion.elapsed() >= Duration::from_secs(15))
            {
                self.driving = false;
                self.radio_revisions.invalidate_all();
                self.front_gap_close = false;
                self.behind_gap_close = false;
                self.front_gap_candidate = None;
                self.behind_gap_candidate = None;
                self.front_gap_clear_since = None;
                self.behind_gap_clear_since = None;
                return EngineerCoreUpdate {
                    calls,
                    timeline_reset,
                };
            }
        }

        let mut started_driving = false;
        if !self.driving {
            let Some(input) = &self.snapshot.input else {
                return EngineerCoreUpdate {
                    calls,
                    timeline_reset,
                };
            };
            if !is_driving(input) {
                return EngineerCoreUpdate {
                    calls,
                    timeline_reset,
                };
            }
            self.driving = true;
            started_driving = true;
            if !self.online_announced {
                self.online_announced = true;
                calls.push(EngineerCall::normal(
                    "engineer_online",
                    "레이스 엔지니어 연결. 텔레메트리 감시를 시작한다.",
                ));
            }
        }

        if started_driving && (update.session.is_none() || !session_is_fresh) {
            self.inspect_session(&mut calls);
            self.inspect_flag(&mut calls);
        }

        if update.lap.is_some() && lap_is_fresh {
            self.inspect_lap(&mut calls);
            self.inspect_flag(&mut calls);
        }
        if update.race_order.is_some() && race_order_is_fresh {
            self.inspect_race_order(&mut calls);
        }
        if update.status.is_some() {
            self.inspect_status(&mut calls);
        }
        if update.damage.is_some() {
            self.inspect_damage(&mut calls);
        }
        if update.session.is_some() && session_is_fresh {
            self.inspect_session(&mut calls);
            self.inspect_flag(&mut calls);
        }
        EngineerCoreUpdate {
            calls,
            timeline_reset,
        }
    }

    fn reset_timeline(&mut self, previous: f32, current: f32) -> TimelineReset {
        self.timeline_revision = self.timeline_revision.saturating_add(1);
        let reset = TimelineReset {
            revision: self.timeline_revision,
            session_uid: self.snapshot.session_uid,
            rollback_from_session_time: previous,
            rollback_to_session_time: current,
        };
        self.last_timeline_reset = Some(reset);
        self.last_session_time = Some(current);
        self.lap_cursor = None;
        self.race_order_cursor = None;
        self.session_cursor = None;
        self.snapshot.reset_timeline();
        self.last_lap_num = None;
        self.last_position = None;
        self.lap_invalid = false;
        self.front_gap_close = false;
        self.behind_gap_close = false;
        self.front_gap_candidate = None;
        self.behind_gap_candidate = None;
        self.front_gap_clear_since = None;
        self.behind_gap_clear_since = None;
        self.pit_limiter = None;
        self.safety_car_status = None;
        self.weather = None;
        self.rain_risk_level = 0;
        self.nearby_pit_status.clear();
        self.nearby_gap_ms.clear();
        self.nearby_pit_announced.clear();
        self.last_front_peer = None;
        self.last_behind_peer = None;
        self.damage_initialized = false;
        self.tyre_wear_level = 0;
        self.tyre_damage_level = 0;
        self.front_wing_level = 0;
        self.rear_wing_level = 0;
        self.gearbox_level = 0;
        self.engine_level = 0;
        self.current_flag = 0;
        self.radio_revisions = RadioStateRevisions::default();
        reset
    }

    fn radio_scope(&self, source: &str) -> RadioScope {
        RadioScope {
            source_key: stable_source_key(source),
            session_uid: self.snapshot.session_uid,
            timeline_revision: self.timeline_revision,
            state_revisions: self.radio_revisions,
            safe_to_speak: self
                .snapshot
                .input
                .as_ref()
                .is_some_and(|input| input.brake < 0.2 && input.steer.abs() < 0.35),
        }
    }

    fn radio_revision(&self, kind: &str) -> u64 {
        self.radio_revisions.revision_for(voice_state_key(kind))
    }

    fn inspect_lap(&mut self, calls: &mut Vec<EngineerCall>) {
        let Some(lap) = self.snapshot.lap.as_ref() else {
            return;
        };
        let position_changed = self
            .last_position
            .is_some_and(|position| position != lap.car_position);

        if lap.current_lap_num > 0 {
            match self.last_lap_num {
                Some(previous) if lap.current_lap_num > previous => {
                    self.lap_invalid = false;
                }
                Some(previous) if lap.current_lap_num < previous => {
                    self.last_position = None;
                    self.lap_invalid = false;
                    self.radio_revisions.position = self.radio_revisions.position.saturating_add(1);
                }
                _ => {}
            }
            self.last_lap_num = Some(lap.current_lap_num);
        }

        if lap.current_lap_invalid && !self.lap_invalid {
            self.radio_revisions.strategy = self.radio_revisions.strategy.saturating_add(1);
            calls.push(EngineerCall::important(
                "lap_invalid",
                "현재 랩 무효. 다음 랩을 준비해.",
            ));
        }
        self.lap_invalid = lap.current_lap_invalid;

        if lap.car_position > 0 {
            if self.last_position != Some(lap.car_position) {
                self.radio_revisions.position = self.radio_revisions.position.saturating_add(1);
            }
            if let Some(previous) = self.last_position
                && previous != lap.car_position
            {
                let movement = if lap.car_position < previous {
                    "순위를 올렸다"
                } else {
                    "순위를 내줬다"
                };
                calls.push(EngineerCall::important(
                    "position",
                    format!("현재 P{}. {}.", lap.car_position, movement),
                ));
            }
            self.last_position = Some(lap.car_position);
        }

        if position_changed {
            self.front_gap_close = false;
            self.behind_gap_close = false;
            self.front_gap_candidate = None;
            self.behind_gap_candidate = None;
            self.front_gap_clear_since = None;
            self.behind_gap_clear_since = None;
            self.radio_revisions.front_gap = self.radio_revisions.front_gap.saturating_add(1);
            self.radio_revisions.behind_gap = self.radio_revisions.behind_gap.saturating_add(1);
            self.radio_revisions.rival = self.radio_revisions.rival.saturating_add(1);
        }

        let gaps_are_actionable = lap.pit_status == 0
            && matches!(lap.driver_status, 1 | 4)
            && lap.result_status == 2
            && self
                .snapshot
                .session
                .as_ref()
                .is_none_or(|session| session.safety_car_status == 0);
        if !gaps_are_actionable {
            self.front_gap_close = false;
            self.behind_gap_close = false;
            self.front_gap_candidate = None;
            self.behind_gap_candidate = None;
            self.front_gap_clear_since = None;
            self.behind_gap_clear_since = None;
            return;
        }

        update_gap(
            calls,
            GapObservation {
                gap_ms: lap
                    .car_in_front_index
                    .zip(lap.delta_to_car_in_front_ms)
                    .map(|(_, gap)| gap),
                session_time: lap.session_time,
                peer_index: lap.car_in_front_index,
                player_position: lap.car_position,
            },
            GapTracker {
                is_close: &mut self.front_gap_close,
                candidate: &mut self.front_gap_candidate,
                clear_since: &mut self.front_gap_clear_since,
                state_revision: &mut self.radio_revisions.front_gap,
            },
            GapRadioText {
                kind: "front_gap",
                car: "앞차",
                context: "공격 가능 거리",
            },
        );
        update_gap(
            calls,
            GapObservation {
                gap_ms: lap
                    .car_behind_index
                    .zip(lap.delta_to_car_behind_ms)
                    .map(|(_, gap)| gap),
                session_time: lap.session_time,
                peer_index: lap.car_behind_index,
                player_position: lap.car_position,
            },
            GapTracker {
                is_close: &mut self.behind_gap_close,
                candidate: &mut self.behind_gap_candidate,
                clear_since: &mut self.behind_gap_clear_since,
                state_revision: &mut self.radio_revisions.behind_gap,
            },
            GapRadioText {
                kind: "behind_gap",
                car: "뒤차",
                context: "방어 거리",
            },
        );
    }

    fn inspect_session(&mut self, calls: &mut Vec<EngineerCall>) {
        let Some(session) = self.snapshot.session.as_ref() else {
            return;
        };
        let safety_car_status = session.safety_car_status;
        let weather = session.weather;
        let rain_probability = session
            .weather_forecast_samples
            .iter()
            .filter(|forecast| {
                forecast.time_offset_min <= 10
                    && (forecast.session_type == 0 || forecast.session_type == session.session_type)
            })
            .map(|forecast| forecast.rain_percentage)
            .max()
            .unwrap_or(0);

        if self.safety_car_status != Some(safety_car_status) {
            self.radio_revisions.race_control = self.radio_revisions.race_control.saturating_add(1);
            self.radio_revisions.rival = self.radio_revisions.rival.saturating_add(1);
            match safety_car_status {
                1 => calls.push(EngineerCall::critical(
                    "safety_car",
                    "세이프티 카. 델타를 지키고 추월 금지. 타이어와 피트 옵션을 확인한다.",
                )),
                2 => calls.push(EngineerCall::critical(
                    "virtual_safety_car",
                    "버추얼 세이프티 카. 델타를 지키고 추월 금지.",
                )),
                0 if matches!(self.safety_car_status, Some(1 | 2)) => {
                    calls.push(EngineerCall::important(
                        "race_restart",
                        "세이프티 카 종료. 재시작 준비, 타이어와 브레이크 온도를 올려.",
                    ));
                }
                _ => {}
            }
            self.safety_car_status = Some(safety_car_status);
        }

        if self.weather != Some(weather) {
            self.radio_revisions.conditions =
                self.radio_revisions.conditions.saturating_add(1);
            match weather {
                3..=5 => calls.push(EngineerCall::important(
                    "rain_started",
                    "비가 시작됐다. 그립 변화를 확인하고 타이어 크로스오버를 판단한다.",
                )),
                0..=2 if self.weather.is_some_and(|previous| previous >= 3) => {
                    calls.push(EngineerCall::normal(
                        "track_drying",
                        "비가 그쳤다. 드라잉 라인과 슬릭 크로스오버를 확인한다.",
                    ));
                }
                _ => {}
            }
            self.weather = Some(weather);
        }

        let rain_risk_level = if rain_probability >= 70 {
            2
        } else if rain_probability >= 40 {
            1
        } else {
            0
        };
        if rain_risk_level != self.rain_risk_level {
            self.radio_revisions.conditions =
                self.radio_revisions.conditions.saturating_add(1);
        }
        if weather < 3 && rain_risk_level > self.rain_risk_level {
            let call = if rain_risk_level >= 2 {
                EngineerCall::important(
                    "weather_forecast",
                    format!(
                        "10분 내 비 확률 {rain_probability}%. 인터미디엇과 피트 진입 타이밍을 준비한다."
                    ),
                )
            } else {
                EngineerCall::normal(
                    "weather_forecast",
                    format!("10분 내 비 확률 {rain_probability}%. 날씨 추세를 감시한다."),
                )
            };
            calls.push(call);
        }
        self.rain_risk_level = if rain_probability < 20 {
            0
        } else {
            self.rain_risk_level.max(rain_risk_level)
        };
    }

    fn inspect_race_order(&mut self, calls: &mut Vec<EngineerCall>) {
        let (Some(order), Some(lap), Some(session)) = (
            self.snapshot.race_order.as_ref(),
            self.snapshot.lap.as_ref(),
            self.snapshot
                .session
                .as_ref()
                .filter(|session| matches!(session.session_type, 15..=17)),
        ) else {
            return;
        };
        let player_is_on_track =
            lap.pit_status == 0 && matches!(lap.driver_status, 1 | 4) && lap.result_status == 2;
        let player_order = order
            .cars
            .iter()
            .find(|car| car.car_index == order.player_car_index);
        let front_peer = lap
            .car_position
            .checked_sub(1)
            .and_then(|position| race_car_at_position(order, position))
            .map(|car| car.car_index);
        let behind_peer = lap
            .car_position
            .checked_add(1)
            .and_then(|position| race_car_at_position(order, position))
            .map(|car| car.car_index);

        if self.last_front_peer != front_peer || self.last_behind_peer != behind_peer {
            self.radio_revisions.rival = self.radio_revisions.rival.saturating_add(1);
        }

        if player_is_on_track {
            let nearby = [
                ("front", self.last_front_peer.or(front_peer)),
                ("behind", self.last_behind_peer.or(behind_peer)),
            ];
            for (relation, peer_index) in nearby {
                let Some(peer_index) = peer_index else {
                    continue;
                };
                let Some(peer) = order.cars.iter().find(|car| car.car_index == peer_index) else {
                    continue;
                };
                let previous_pit_status = self.nearby_pit_status.get(&peer_index).copied();
                let gap_ms = self.nearby_gap_ms.get(&peer_index).copied().or_else(|| {
                    if relation == "front" {
                        lap.delta_to_car_in_front_ms
                    } else {
                        peer.delta_to_car_in_front_ms
                    }
                });
                let same_lap = player_order
                    .is_some_and(|player| player.current_lap_num == peer.current_lap_num);
                let pit_cycle = (peer.current_lap_num, peer.num_pit_stops);
                let already_announced =
                    self.nearby_pit_announced.get(&peer_index) == Some(&pit_cycle);
                if previous_pit_status == Some(0)
                    && peer.pit_status != 0
                    && same_lap
                    && gap_ms.is_some_and(|gap| gap <= RIVAL_PIT_MAX_GAP_MS)
                    && !already_announced
                {
                    self.nearby_pit_announced.insert(peer_index, pit_cycle);
                    let gap = gap_ms.unwrap_or_default() as f32 / 1_000.0;
                    let call = if session.safety_car_status != 0 {
                        EngineerCall::important(
                            "rival_pit_safety_car",
                            format!(
                                "가까운 {relation_label}가 세이프티 카 중 피트 진입. 간격 {gap:.1}초, 트랙 포지션과 피트 옵션을 확인한다.",
                                relation_label = if relation == "front" {
                                    "앞차"
                                } else {
                                    "뒤차"
                                }
                            ),
                        )
                    } else if relation == "front" {
                        EngineerCall::important(
                            "rival_pit_front",
                            format!(
                                "앞차 {gap:.1}초에서 피트 진입. 클린에어를 사용해 푸시하고 오버컷 가능성을 확인한다."
                            ),
                        )
                    } else {
                        EngineerCall::important(
                            "rival_pit_behind",
                            format!(
                                "뒤차 {gap:.1}초에서 피트 진입. 언더컷 가능성이다. 타이어와 피트 윈도를 확인한다."
                            ),
                        )
                    };
                    calls.push(call);
                }
            }
        }

        self.nearby_pit_status.clear();
        self.nearby_pit_status
            .extend(order.cars.iter().map(|car| (car.car_index, car.pit_status)));
        if let Some(peer_index) = front_peer
            && let Some(gap) = lap.delta_to_car_in_front_ms
        {
            self.nearby_gap_ms.insert(peer_index, gap);
        }
        if let Some(peer_index) = behind_peer
            && let Some(peer) = order.cars.iter().find(|car| car.car_index == peer_index)
            && let Some(gap) = peer.delta_to_car_in_front_ms.or(lap.delta_to_car_behind_ms)
        {
            self.nearby_gap_ms.insert(peer_index, gap);
        }
        self.last_front_peer = front_peer;
        self.last_behind_peer = behind_peer;
    }

    fn inspect_status(&mut self, calls: &mut Vec<EngineerCall>) {
        let Some(status) = self.snapshot.status.as_ref() else {
            return;
        };

        if let Some(previous) = self.pit_limiter
            && previous != status.pit_limiter_active
        {
            self.radio_revisions.pit = self.radio_revisions.pit.saturating_add(1);
            let message = if status.pit_limiter_active {
                "피트 리미터 작동."
            } else {
                "피트 리미터 해제."
            };
            calls.push(EngineerCall::important("pit_limiter", message));
        }
        self.pit_limiter = Some(status.pit_limiter_active);
    }

    fn inspect_final_classification(&mut self, calls: &mut Vec<EngineerCall>) {
        let Some(final_classification) = self.snapshot.final_classification.as_ref() else {
            return;
        };
        if self.final_announced {
            return;
        }
        self.final_announced = true;
        self.driving = false;
        self.radio_revisions.invalidate_all();
        let message = match final_classification.result_status {
            3 => format!(
                "체커드 플래그. {}랩 완주, 최종 P{}.",
                final_classification.num_laps, final_classification.position
            ),
            4 | 7 => format!(
                "세션 종료. 최종 P{}, 완주하지 못했다.",
                final_classification.position
            ),
            5 => "세션 종료. 실격 처리됐다.".to_owned(),
            _ => format!("세션 종료. 최종 P{}.", final_classification.position),
        };
        calls.push(EngineerCall::important("session_finished", message));
    }

    fn inspect_damage(&mut self, calls: &mut Vec<EngineerCall>) {
        let Some(damage) = self.snapshot.damage.as_ref() else {
            return;
        };

        if !self.damage_initialized {
            self.damage_initialized = true;
            self.tyre_wear_level = rising_level(max_wheel(damage.tyre_wear).0, [55.0, 70.0, 85.0]);
            self.tyre_damage_level =
                rising_level(max_excess_tyre_damage(damage), [10.0, 35.0, 70.0]);
            self.front_wing_level = rising_level(
                damage
                    .front_left_wing_damage
                    .max(damage.front_right_wing_damage) as f32,
                [10.0, 35.0, 70.0],
            );
            self.rear_wing_level = rising_level(damage.rear_wing_damage as f32, [10.0, 35.0, 70.0]);
            self.gearbox_level = rising_level(damage.gearbox_damage as f32, [10.0, 35.0, 70.0]);
            self.engine_level = rising_level(damage.engine_damage as f32, [10.0, 35.0, 70.0]);
            return;
        }

        let previous_levels = (
            self.tyre_wear_level,
            self.tyre_damage_level,
            self.front_wing_level,
            self.rear_wing_level,
            self.gearbox_level,
            self.engine_level,
        );

        let (max_wear, corner) = max_wheel(damage.tyre_wear);
        if max_wear >= 0.0 {
            let level = rising_level(max_wear, [55.0, 70.0, 85.0]);
            if level > self.tyre_wear_level {
                let call = match level {
                    3 => EngineerCall::critical(
                        "tyre_wear",
                        format!("{corner} 타이어 마모 {:.0}퍼센트. 피트 준비.", max_wear),
                    ),
                    2 => EngineerCall::important(
                        "tyre_wear",
                        format!("{corner} 타이어 마모 {:.0}퍼센트. 그립 관리해.", max_wear),
                    ),
                    _ => EngineerCall::normal(
                        "tyre_wear",
                        format!("타이어 마모 증가. {corner} {:.0}퍼센트.", max_wear),
                    ),
                };
                calls.push(call);
            }
            self.tyre_wear_level = level;
        }

        let max_tyre_damage = max_excess_tyre_damage(damage);
        let tyre_damage_level = rising_level(max_tyre_damage, [10.0, 35.0, 70.0]);
        if tyre_damage_level > self.tyre_damage_level {
            let call = if tyre_damage_level >= 3 {
                EngineerCall::critical("tyre_damage", "타이어 손상 심각. 즉시 피트 진입 권장.")
            } else {
                EngineerCall::important("tyre_damage", "타이어 손상 감지. 상태 확인해.")
            };
            calls.push(call);
        }
        self.tyre_damage_level = tyre_damage_level;

        let front_wing = damage
            .front_left_wing_damage
            .max(damage.front_right_wing_damage) as f32;
        inspect_component_damage(
            calls,
            "front_wing_damage",
            "프런트 윙",
            front_wing,
            &mut self.front_wing_level,
        );
        inspect_component_damage(
            calls,
            "rear_wing_damage",
            "리어 윙",
            damage.rear_wing_damage as f32,
            &mut self.rear_wing_level,
        );
        inspect_component_damage(
            calls,
            "gearbox_damage",
            "기어박스",
            damage.gearbox_damage as f32,
            &mut self.gearbox_level,
        );
        inspect_component_damage(
            calls,
            "engine_damage",
            "엔진",
            damage.engine_damage as f32,
            &mut self.engine_level,
        );
        let current_levels = (
            self.tyre_wear_level,
            self.tyre_damage_level,
            self.front_wing_level,
            self.rear_wing_level,
            self.gearbox_level,
            self.engine_level,
        );
        if current_levels != previous_levels {
            self.radio_revisions.damage = self.radio_revisions.damage.saturating_add(1);
        }
    }

    fn inspect_flag(&mut self, calls: &mut Vec<EngineerCall>) {
        let (Some(session), Some(lap)) =
            (self.snapshot.session.as_ref(), self.snapshot.lap.as_ref())
        else {
            return;
        };
        let next_flag = current_marshal_flag(session, lap);
        if next_flag == self.current_flag {
            return;
        }
        self.radio_revisions.track_flag = self.radio_revisions.track_flag.saturating_add(1);

        match next_flag {
            3 => calls.push(EngineerCall::critical(
                "yellow_flag",
                "옐로 플래그. 감속하고 추월 금지.",
            )),
            4 => calls.push(EngineerCall::critical(
                "red_flag",
                "레드 플래그. 즉시 감속.",
            )),
            0 | 1 if matches!(self.current_flag, 3 | 4) => calls.push(EngineerCall::normal(
                "green_flag",
                "그린 플래그. 레이스 재개.",
            )),
            _ => {}
        }
        self.current_flag = next_flag;
    }
}

fn race_car_at_position(order: &RaceOrderSample, position: u8) -> Option<&RaceOrderCarSample> {
    if position == 0 {
        return None;
    }
    let mut matches = order.cars.iter().filter(|car| {
        car.car_position == position && car.result_status == 2 && matches!(car.driver_status, 1..=4)
    });
    let matched = matches.next()?;
    matches.next().is_none().then_some(matched)
}

fn max_excess_tyre_damage(damage: &DamageSample) -> f32 {
    [
        damage.tyre_damage.fl as f32 - damage.tyre_wear.fl,
        damage.tyre_damage.fr as f32 - damage.tyre_wear.fr,
        damage.tyre_damage.rl as f32 - damage.tyre_wear.rl,
        damage.tyre_damage.rr as f32 - damage.tyre_wear.rr,
    ]
    .into_iter()
    .fold(0.0, f32::max)
}

fn max_tyre_blister(damage: &DamageSample) -> u8 {
    [
        damage.tyre_blisters.fl,
        damage.tyre_blisters.fr,
        damage.tyre_blisters.rl,
        damage.tyre_blisters.rr,
    ]
    .into_iter()
    .max()
    .unwrap_or(0)
}

fn newest_session_time(update: &TelemetryUpdate) -> Option<f32> {
    [
        update.input.as_ref().map(|sample| sample.session_time),
        update.lap.as_ref().map(|sample| sample.session_time),
        update.race_order.as_ref().map(|sample| sample.session_time),
        update.session.as_ref().map(|sample| sample.session_time),
        update.damage.as_ref().map(|sample| sample.session_time),
        update.status.as_ref().map(|sample| sample.session_time),
        update.setup.as_ref().map(|sample| sample.session_time),
        update.tyre_sets.as_ref().map(|sample| sample.session_time),
        update
            .final_classification
            .as_ref()
            .map(|sample| sample.session_time),
    ]
    .into_iter()
    .flatten()
    .filter(|value| value.is_finite() && *value > 0.0)
    .max_by(|left, right| left.total_cmp(right))
}

fn is_driving(input: &InputSample) -> bool {
    input.speed_kmh >= 10
        || (input.rpm >= 800 && (input.throttle >= 0.15 || !matches!(input.gear, -1 | 0)))
}

fn update_gap(
    calls: &mut Vec<EngineerCall>,
    observation: GapObservation,
    tracker: GapTracker<'_>,
    radio: GapRadioText,
) {
    let was_close = *tracker.is_close;
    let Some(gap_ms) = observation.gap_ms.filter(|gap| *gap > 0) else {
        *tracker.candidate = None;
        *tracker.clear_since = None;
        return;
    };
    let is_close = if gap_ms <= GAP_ALERT_MS {
        *tracker.clear_since = None;
        if was_close {
            *tracker.candidate = None;
            true
        } else if !observation.session_time.is_finite() {
            *tracker.candidate = None;
            false
        } else if tracker.candidate.is_some_and(|candidate| {
            candidate.peer_index == observation.peer_index
                && candidate.player_position == observation.player_position
                && observation.session_time >= candidate.since
                && observation.session_time - candidate.since >= GAP_CONFIRM_S
        }) {
            calls.push(EngineerCall::important(
                radio.kind,
                format!(
                    "{} {:.1}초. {}.",
                    radio.car,
                    gap_ms as f32 / 1_000.0,
                    radio.context
                ),
            ));
            *tracker.candidate = None;
            true
        } else {
            if tracker.candidate.is_none_or(|candidate| {
                observation.session_time < candidate.since
                    || candidate.peer_index != observation.peer_index
                    || candidate.player_position != observation.player_position
            }) {
                *tracker.candidate = Some(GapCandidate {
                    since: observation.session_time,
                    peer_index: observation.peer_index,
                    player_position: observation.player_position,
                });
            }
            false
        }
    } else if gap_ms >= GAP_CLEAR_MS {
        *tracker.candidate = None;
        if !was_close
            || (observation.session_time.is_finite()
                && tracker.clear_since.is_some_and(|started| {
                    observation.session_time >= started
                        && observation.session_time - started >= GAP_CLEAR_CONFIRM_S
                }))
        {
            *tracker.clear_since = None;
            false
        } else {
            if observation.session_time.is_finite()
                && tracker
                    .clear_since
                    .is_none_or(|started| observation.session_time < started)
            {
                *tracker.clear_since = Some(observation.session_time);
            }
            true
        }
    } else {
        *tracker.candidate = None;
        *tracker.clear_since = None;
        was_close
    };
    if is_close != was_close {
        *tracker.state_revision = tracker.state_revision.saturating_add(1);
        *tracker.is_close = is_close;
    }
}

fn rising_level(value: f32, thresholds: [f32; 3]) -> u8 {
    if value >= thresholds[2] {
        3
    } else if value >= thresholds[1] {
        2
    } else if value >= thresholds[0] {
        1
    } else {
        0
    }
}

fn max_wheel(values: WheelValuesF32) -> (f32, &'static str) {
    [
        (values.fl, "왼쪽 앞"),
        (values.fr, "오른쪽 앞"),
        (values.rl, "왼쪽 뒤"),
        (values.rr, "오른쪽 뒤"),
    ]
    .into_iter()
    .max_by(|left, right| left.0.total_cmp(&right.0))
    .unwrap_or((-1.0, ""))
}

fn inspect_component_damage(
    calls: &mut Vec<EngineerCall>,
    kind: &'static str,
    component: &str,
    damage: f32,
    previous_level: &mut u8,
) {
    let level = rising_level(damage, [10.0, 35.0, 70.0]);
    if level > *previous_level {
        let call = match level {
            3 => EngineerCall::critical(
                kind,
                format!("{component} 손상 {:.0}퍼센트. 피트 권장.", damage),
            ),
            2 => EngineerCall::important(
                kind,
                format!("{component} 손상 {:.0}퍼센트. 페이스 조절해.", damage),
            ),
            _ => EngineerCall::normal(
                kind,
                format!("{component} 경미한 손상 감지. {:.0}퍼센트.", damage),
            ),
        };
        calls.push(call);
    }
    *previous_level = level;
}

fn current_marshal_flag(session: &SessionSample, lap: &LapSample) -> i8 {
    if session.track_length_m == 0 || lap.lap_distance_m < 0.0 {
        return 0;
    }
    let position = (lap.lap_distance_m / session.track_length_m as f32).clamp(0.0, 1.0);
    session
        .marshal_zones
        .iter()
        .filter(|zone| zone.start.is_finite() && zone.start <= position)
        .max_by(|left, right| left.start.total_cmp(&right.start))
        .map(|zone| zone.flag)
        .unwrap_or(0)
}

fn format_lap_time(time_ms: u32) -> String {
    let minutes = time_ms / 60_000;
    let seconds = (time_ms % 60_000) / 1_000;
    let millis = time_ms % 1_000;
    format!("{minutes}:{seconds:02}.{millis:03}")
}
