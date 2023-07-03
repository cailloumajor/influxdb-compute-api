use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use hex_color::HexColor;
use serde::ser::SerializeTuple;
use serde::{Serialize, Serializer};

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
    #[serde(serialize_with = "serialize_maybe_color")]
    pub(crate) color: Option<HexColor>,
}

fn serialize_maybe_color<S>(value: &Option<HexColor>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        Some(color) => {
            let mut tup = serializer.serialize_tuple(3)?;
            tup.serialize_element(&color.r)?;
            tup.serialize_element(&color.g)?;
            tup.serialize_element(&color.b)?;
            tup.end()
        }
        None => serializer.serialize_none(),
    }
}
