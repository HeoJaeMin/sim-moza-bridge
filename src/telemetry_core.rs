//! Game-neutral capture, session, and analysis models.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct CaptureCounters {
    pub accepted_frames: u64,
    pub rejected_frames: u64,
    pub duplicate_frames: u64,
    pub stalled_frames: u64,
    pub inconsistent_frames: u64,
    pub invalid_context_frames: u64,
    pub persistence_dropped_frames: u64,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct SessionIdentity {
    source: String,
    track_name: String,
    session_type: String,
    vehicle_name: Option<String>,
    track_length_m: Option<i64>,
    game_version: Option<i32>,
    max_laps: Option<i32>,
}

impl SessionIdentity {
    pub fn new(
        source: impl Into<String>,
        track_name: impl Into<String>,
        session_type: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into().trim().to_owned(),
            track_name: track_name.into().trim().to_owned(),
            session_type: session_type.into().trim().to_owned(),
            ..Self::default()
        }
    }

    pub fn with_vehicle(mut self, vehicle_name: impl Into<String>) -> Self {
        let vehicle_name = vehicle_name.into();
        let vehicle_name = vehicle_name.trim();
        self.vehicle_name = (!vehicle_name.is_empty()).then(|| vehicle_name.to_owned());
        self
    }

    pub fn with_track_length(mut self, track_length_m: f64) -> Self {
        self.track_length_m = track_length_m
            .is_finite()
            .then(|| track_length_m.round() as i64);
        self
    }

    pub fn with_game_version(mut self, game_version: i32) -> Self {
        self.game_version = Some(game_version);
        self
    }

    pub fn with_max_laps(mut self, max_laps: i32) -> Self {
        self.max_laps = Some(max_laps);
        self
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn track_name(&self) -> &str {
        &self.track_name
    }

    pub fn session_type(&self) -> &str {
        &self.session_type
    }

    pub fn vehicle_name(&self) -> Option<&str> {
        self.vehicle_name.as_deref()
    }

    pub fn is_complete(&self) -> bool {
        !self.source.is_empty()
            && !self.track_name.is_empty()
            && !self.track_name.eq_ignore_ascii_case("Waiting for track")
            && !self.track_name.eq_ignore_ascii_case("Unknown")
            && !self.session_type.is_empty()
            && !self.session_type.eq_ignore_ascii_case("Unknown")
    }

    /// Keeps the existing LMU fingerprint layout for database compatibility.
    pub fn fingerprint(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.track_key(),
            normalized_component(&self.session_type),
            self.game_version.unwrap_or_default(),
            self.max_laps.unwrap_or_default()
        )
    }

    pub fn boundary_key(&self) -> String {
        format!(
            "{}:{}",
            self.track_key(),
            normalized_component(&self.session_type)
        )
    }

    pub fn storage_slug(&self) -> String {
        let mut parts = vec![slug_component(&self.track_name, 48)];
        if let Some(vehicle_name) = &self.vehicle_name {
            parts.push(slug_component(vehicle_name, 48));
        }
        parts.join("-")
    }

    pub fn track_key(&self) -> String {
        track_key(
            &self.track_name,
            self.track_length_m.unwrap_or_default() as f64,
        )
    }
}

pub fn track_key(track_name: &str, track_length_m: f64) -> String {
    format!(
        "{}-{}",
        normalized_slug(track_name, usize::MAX),
        track_length_m.round() as i64
    )
}

fn normalized_component(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn slug_component(value: &str, max_chars: usize) -> String {
    let slug = normalized_slug(value, max_chars);
    if slug.is_empty() {
        "unknown".to_owned()
    } else {
        slug
    }
}

fn normalized_slug(value: &str, max_chars: usize) -> String {
    let mut slug = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    slug.chars().take(max_chars).collect()
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisConfidenceLevel {
    #[default]
    Unknown,
    Low,
    Medium,
    High,
}

impl fmt::Display for AnalysisConfidenceLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Unknown => "unknown",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisLimitation {
    TrafficNotAdjusted,
    SlipstreamNotAdjusted,
    FuelLoadNotAdjusted,
    TyreStateNotAdjusted,
    LimitedCommonSamples,
    PartialCommonDistance,
    ComparisonIncludesFailedOrInvalidAttempt,
    ValidatorDropsPresent,
    ArchiveBackpressureDropsPresent,
}

impl AnalysisLimitation {
    pub const ENVIRONMENTAL_DEFAULTS: [Self; 4] = [
        Self::TrafficNotAdjusted,
        Self::SlipstreamNotAdjusted,
        Self::FuelLoadNotAdjusted,
        Self::TyreStateNotAdjusted,
    ];

    pub fn description_ko(self) -> &'static str {
        match self {
            Self::TrafficNotAdjusted => "교통량 영향은 보정하지 않았습니다.",
            Self::SlipstreamNotAdjusted => "슬립스트림 영향은 보정하지 않았습니다.",
            Self::FuelLoadNotAdjusted => "연료량 차이는 보정하지 않았습니다.",
            Self::TyreStateNotAdjusted => "타이어 상태와 컴파운드 차이는 보정하지 않았습니다.",
            Self::LimitedCommonSamples => "공통 비교 샘플 수가 부족합니다.",
            Self::PartialCommonDistance => "두 주행의 공통 비교 거리가 짧습니다.",
            Self::ComparisonIncludesFailedOrInvalidAttempt => {
                "실패 또는 무효 주행이 비교에 포함되었습니다."
            }
            Self::ValidatorDropsPresent => "검증기에서 제외된 프레임이 있습니다.",
            Self::ArchiveBackpressureDropsPresent => "저장 대기열 포화로 제외된 프레임이 있습니다.",
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct AnalysisConfidence {
    pub score: f32,
    pub level: AnalysisConfidenceLevel,
    pub limitations: Vec<AnalysisLimitation>,
}

impl AnalysisConfidence {
    pub fn from_score(score: f32, limitations: Vec<AnalysisLimitation>) -> Self {
        let score = if score.is_finite() {
            score.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let level = if score >= 0.8 {
            AnalysisConfidenceLevel::High
        } else if score >= 0.5 {
            AnalysisConfidenceLevel::Medium
        } else {
            AnalysisConfidenceLevel::Low
        };
        Self {
            score,
            level,
            limitations,
        }
    }

    pub fn for_cohort(participants: usize, repeated_player_laps: usize) -> Self {
        let score = if participants >= 8 && repeated_player_laps >= 2 {
            0.9
        } else if participants >= 4 {
            0.65
        } else {
            0.35
        };
        Self::from_score(score, Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_the_existing_lmu_fingerprint_format() {
        let identity = SessionIdentity::new("lmu", "Le Mans 2024", " Race ")
            .with_track_length(13_626.2)
            .with_game_version(17)
            .with_max_laps(42);

        assert_eq!(identity.fingerprint(), "le-mans-2024-13626:race:17:42");
        assert_eq!(identity.boundary_key(), "le-mans-2024-13626:race");
        assert_eq!(track_key("한글", 0.0), "-0");
    }

    #[test]
    fn keeps_utf8_identity_values_and_builds_a_safe_storage_slug() {
        let identity =
            SessionIdentity::new("acr", "Alsace Forêt", "stage").with_vehicle("랠리 차량 A");

        assert_eq!(identity.track_name(), "Alsace Forêt");
        assert_eq!(identity.vehicle_name(), Some("랠리 차량 A"));
        assert_eq!(identity.storage_slug(), "alsace-for-t-a");
        assert!(!SessionIdentity::new("lmu", "Waiting for track", "Race").is_complete());
        assert!(!SessionIdentity::new("lmu", "Le Mans", "Unknown").is_complete());
    }

    #[test]
    fn assigns_consistent_confidence_levels() {
        assert_eq!(
            AnalysisConfidence::for_cohort(8, 2).level,
            AnalysisConfidenceLevel::High
        );
        assert_eq!(
            AnalysisConfidence::for_cohort(4, 1).level,
            AnalysisConfidenceLevel::Medium
        );
        assert_eq!(
            AnalysisConfidence::from_score(f32::NAN, Vec::new()).level,
            AnalysisConfidenceLevel::Low
        );
        assert_eq!(
            serde_json::to_string(&AnalysisConfidenceLevel::High).unwrap(),
            "\"high\""
        );
    }
}
