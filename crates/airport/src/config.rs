//! The fixed description of an airport: its runways and operating limits.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::Ms;
use crate::data::{
    DEFAULT_CROSSWIND_LIMIT_KT, DEFAULT_FINAL_APPROACH_NM, DEFAULT_MAX_ARRIVAL_HOLD_MS,
    DEFAULT_MAYDAY_INSPECTION_MS,
};

/// Most runways one airport may have. Keeps the search for the best set of
/// runways small.
pub const MAX_RUNWAYS: usize = 12;

/// One strip of tarmac, usable in either direction.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunwayConfig {
    /// Name of the lower-numbered end, e.g. `09L`.
    pub low: String,
    /// Name of the opposite end, e.g. `27R`.
    pub high: String,
    /// Heading in degrees true when using the `low` end. The `high` end is
    /// 180 degrees round.
    pub heading_deg: u16,
    pub length_m: u32,
    /// Extra time an arrival takes to reach an exit on this runway, on top
    /// of the standard runway occupancy time for its class.
    #[serde(default)]
    pub exit_extra_ms: Ms,
    /// The runway is not used when the crosswind is above this.
    #[serde(default = "default_crosswind_limit")]
    pub crosswind_limit_kt: u16,
}

fn default_crosswind_limit() -> u16 {
    DEFAULT_CROSSWIND_LIMIT_KT
}
fn default_final_approach() -> u32 {
    DEFAULT_FINAL_APPROACH_NM
}
fn default_inspection() -> Ms {
    DEFAULT_MAYDAY_INSPECTION_MS
}
fn default_hold() -> Option<Ms> {
    Some(DEFAULT_MAX_ARRIVAL_HOLD_MS)
}

impl RunwayConfig {
    #[must_use]
    pub fn new(low: &str, high: &str, heading_deg: u16, length_m: u32) -> Self {
        Self {
            low: low.to_owned(),
            high: high.to_owned(),
            heading_deg,
            length_m,
            exit_extra_ms: 0,
            crosswind_limit_kt: DEFAULT_CROSSWIND_LIMIT_KT,
        }
    }

    /// The full name, e.g. `09L/27R`.
    #[must_use]
    pub fn name(&self) -> String {
        format!("{}/{}", self.low, self.high)
    }

    /// Whether `name` is this runway's full name or either end.
    #[must_use]
    pub fn is_named(&self, name: &str) -> bool {
        name == self.low
            || name == self.high
            || name
                .split_once('/')
                .is_some_and(|(a, b)| a == self.low && b == self.high)
    }
}

/// An airport as configured before the run starts.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AirportConfig {
    pub name: String,
    pub runways: Vec<RunwayConfig>,
    /// Pairs of runways that cross and so are never in use together, each
    /// named by either end or in full.
    #[serde(default)]
    pub crossings: Vec<(String, String)>,
    /// Length of the common final approach path, in nautical miles.
    #[serde(default = "default_final_approach")]
    pub final_approach_nm: u32,
    /// How long a runway stays shut for inspection after a mayday landing.
    #[serde(default = "default_inspection")]
    pub mayday_inspection_ms: Ms,
    /// How long an ordinary arrival waits for a slot before it diverts.
    /// `None` means arrivals wait for ever.
    #[serde(default = "default_hold")]
    pub max_arrival_hold_ms: Option<Ms>,
}

/// What is wrong with an [`AirportConfig`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    NoRunways,
    TooManyRunways,
    DuplicateRunwayName(String),
    BadHeading(String),
    UnknownCrossing(String),
    SelfCrossing(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRunways => write!(f, "an airport needs at least one runway"),
            Self::TooManyRunways => write!(f, "an airport may have at most {MAX_RUNWAYS} runways"),
            Self::DuplicateRunwayName(n) => write!(f, "runway end name {n} is used twice"),
            Self::BadHeading(n) => write!(f, "runway {n} has a heading of 360 or more"),
            Self::UnknownCrossing(n) => write!(f, "crossing names unknown runway {n}"),
            Self::SelfCrossing(n) => write!(f, "runway {n} cannot cross itself"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl AirportConfig {
    #[must_use]
    pub fn new(name: &str, runways: Vec<RunwayConfig>) -> Self {
        Self {
            name: name.to_owned(),
            runways,
            crossings: Vec::new(),
            final_approach_nm: DEFAULT_FINAL_APPROACH_NM,
            mayday_inspection_ms: DEFAULT_MAYDAY_INSPECTION_MS,
            max_arrival_hold_ms: Some(DEFAULT_MAX_ARRIVAL_HOLD_MS),
        }
    }

    /// One long east-west runway, and arrivals that never give up.
    #[must_use]
    pub fn single_runway() -> Self {
        let mut config = Self::new("Single", vec![RunwayConfig::new("09", "27", 90, 3_500)]);
        config.max_arrival_hold_ms = None;
        config
    }

    /// The demonstration airport: two parallel east-west runways and a
    /// shorter north-south runway that crosses both.
    #[must_use]
    pub fn demo() -> Self {
        let mut south = RunwayConfig::new("09R", "27L", 90, 3_660);
        south.exit_extra_ms = 5_000;
        let mut config = Self::new(
            "Demo International",
            vec![
                RunwayConfig::new("09L", "27R", 90, 3_900),
                south,
                RunwayConfig::new("18", "36", 180, 2_400),
            ],
        );
        config.crossings = vec![
            ("18".to_owned(), "09L".to_owned()),
            ("18".to_owned(), "09R".to_owned()),
        ];
        config
    }

    /// Position of the runway called `name` (either end, or the full name).
    #[must_use]
    pub fn runway_index(&self, name: &str) -> Option<usize> {
        self.runways.iter().position(|r| r.is_named(name))
    }

    /// For each runway, a bit mask of the runways that cross it.
    ///
    /// # Errors
    /// Returns the first problem found with the configuration.
    pub fn validate(&self) -> Result<Vec<u16>, ConfigError> {
        if self.runways.is_empty() {
            return Err(ConfigError::NoRunways);
        }
        if self.runways.len() > MAX_RUNWAYS {
            return Err(ConfigError::TooManyRunways);
        }
        let mut names = BTreeSet::new();
        for runway in &self.runways {
            if runway.heading_deg >= 360 {
                return Err(ConfigError::BadHeading(runway.name()));
            }
            for end in [&runway.low, &runway.high] {
                if !names.insert(end.as_str()) {
                    return Err(ConfigError::DuplicateRunwayName(end.clone()));
                }
            }
        }
        let mut masks = vec![0u16; self.runways.len()];
        for (a, b) in &self.crossings {
            let ia = self
                .runway_index(a)
                .ok_or_else(|| ConfigError::UnknownCrossing(a.clone()))?;
            let ib = self
                .runway_index(b)
                .ok_or_else(|| ConfigError::UnknownCrossing(b.clone()))?;
            if ia == ib {
                return Err(ConfigError::SelfCrossing(a.clone()));
            }
            masks[ia] |= 1 << ib;
            masks[ib] |= 1 << ia;
        }
        Ok(masks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid() {
        assert_eq!(AirportConfig::single_runway().validate(), Ok(vec![0]));
        assert_eq!(
            AirportConfig::demo().validate(),
            Ok(vec![0b100, 0b100, 0b011])
        );
    }

    #[test]
    fn runways_are_found_by_any_name() {
        let config = AirportConfig::demo();
        assert_eq!(config.runway_index("27L"), Some(1));
        assert_eq!(config.runway_index("09R"), Some(1));
        assert_eq!(config.runway_index("09R/27L"), Some(1));
        assert_eq!(config.runway_index("09R/27R"), None);
        assert_eq!(config.runway_index("04"), None);
    }

    #[test]
    fn bad_configurations_are_refused() {
        let mut config = AirportConfig::demo();
        config.crossings.push(("18".into(), "04".into()));
        assert_eq!(
            config.validate(),
            Err(ConfigError::UnknownCrossing("04".into()))
        );
        let mut config = AirportConfig::demo();
        config.runways[2].low = "09L".into();
        assert_eq!(
            config.validate(),
            Err(ConfigError::DuplicateRunwayName("09L".into()))
        );
        assert_eq!(
            AirportConfig::new("None", vec![]).validate(),
            Err(ConfigError::NoRunways)
        );
    }

    #[test]
    fn optional_fields_have_defaults_in_json() {
        let json =
            r#"{"name":"X","runways":[{"low":"09","high":"27","heading_deg":90,"length_m":3000}]}"#;
        let config: AirportConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.runways[0].crosswind_limit_kt, 25);
        assert_eq!(config.max_arrival_hold_ms, Some(45 * 60_000));
        assert_eq!(config.final_approach_nm, 6);
    }
}
