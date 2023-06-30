use chrono::serde::ts_seconds;
use chrono::{DateTime, Utc};
use hex_color::HexColor;
use serde::ser::SerializeTuple;
use serde::{Serialize, Serializer};

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
