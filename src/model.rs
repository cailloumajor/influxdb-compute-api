use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use serde::Serialize;

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
