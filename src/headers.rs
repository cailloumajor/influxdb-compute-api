use axum::http::{HeaderName, HeaderValue};
use axum_extra::headers::{self, Header};
use chrono_tz::Tz;

static CLIENT_TIMEZONE_HEADER_NAME: HeaderName = HeaderName::from_static("client-timezone");

pub(crate) struct ClientTimezone(Tz);

impl ClientTimezone {
    pub(crate) fn into_inner(self) -> Tz {
        self.0
    }
}

impl Header for ClientTimezone {
    fn name() -> &'static HeaderName {
        &CLIENT_TIMEZONE_HEADER_NAME
    }

    fn decode<'i, I>(values: &mut I) -> Result<Self, headers::Error>
    where
        Self: Sized,
        I: Iterator<Item = &'i HeaderValue>,
    {
        values
            .next()
            .ok_or_else(headers::Error::invalid)?
            .to_str()
            .map_err(|_| headers::Error::invalid())?
            .parse::<Tz>()
            .map_err(|_| headers::Error::invalid())
            .map(Self)
    }

    fn encode<E: Extend<HeaderValue>>(&self, _values: &mut E) {
        unimplemented!();
    }
}
