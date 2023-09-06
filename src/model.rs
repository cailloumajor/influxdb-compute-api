use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigFromApi {
    pub(crate) target_cycle_time: f32,
    pub(crate) target_efficiency: f32,
}

#[derive(Debug)]
pub(crate) struct TimelineResponse(Vec<TimelineSlot>);

impl From<Vec<TimelineSlot>> for TimelineResponse {
    fn from(value: Vec<TimelineSlot>) -> Self {
        Self(value)
    }
}

impl TimelineResponse {
    pub(crate) fn into_inner(self) -> Vec<TimelineSlot> {
        self.0
    }
}

#[derive(Debug, PartialEq, Serialize)]
pub(crate) struct TimelineSlot {
    #[serde(with = "ts_seconds")]
    pub(crate) start: DateTime<Utc>,
    pub(crate) color: Option<u8>,
}
