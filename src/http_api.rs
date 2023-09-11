use axum::extract::{Path, Query, State};
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use bytes::{BufMut, BytesMut};
use chrono::{DateTime, FixedOffset};
use reqwest::{header, StatusCode};
use serde::Deserialize;
use tracing::{error, instrument};

use crate::config_api::{ConfigChannel, ConfigRequest};
use crate::influxdb::{
    HealthChannel, PerformanceChannel, PerformanceRequest, TimelineChannel, TimelineRequest,
};
use crate::model::{ConfigFromApi, TimelineResponse};

type HandlerError = (StatusCode, &'static str);

const INTERNAL_ERROR: HandlerError = (StatusCode::INTERNAL_SERVER_ERROR, "internal server error");

impl IntoResponse for TimelineResponse {
    // Taken from axum::Json::into_response
    fn into_response(self) -> Response {
        let mut buf = BytesMut::with_capacity(128).writer();
        match rmp_serde::encode::write(&mut buf, &self.into_inner()) {
            Ok(()) => (
                [(
                    header::CONTENT_TYPE,
                    HeaderValue::from_static(mime::APPLICATION_MSGPACK.as_ref()),
                )],
                buf.into_inner().freeze(),
            )
                .into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PerformanceQueryParams {
    client_time: DateTime<FixedOffset>,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) health_channel: HealthChannel,
    pub(crate) config_channel: ConfigChannel,
    pub(crate) timeline_channel: TimelineChannel,
    pub(crate) performance_channel: PerformanceChannel,
}

pub(crate) fn app(state: AppState) -> Router {
    Router::new()
        .route("/health", routing::get(health_handler))
        .route("/timeline/:id", routing::get(timeline_handler))
        .route("/performance/:id", routing::get(performance_handler))
        .with_state(state)
}

#[instrument(name = "health_api_handler", skip_all)]
async fn health_handler(State(state): State<AppState>) -> Result<StatusCode, HandlerError> {
    state.health_channel.roundtrip(()).await.map_err(|err| {
        error!(kind = "health channel roundtrip", %err);
        INTERNAL_ERROR
    })
}

#[instrument(name = "timeline_api_handler", skip_all)]
async fn timeline_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<TimelineResponse, HandlerError> {
    let config_request = ConfigRequest { id: id.clone() };
    let ConfigFromApi {
        target_cycle_time, ..
    } = state
        .config_channel
        .roundtrip(config_request)
        .await
        .map_err(|err| {
            error!(kind = "config channel roundtrip", %err);
            INTERNAL_ERROR
        })?;
    let timeline_request = TimelineRequest {
        id,
        target_cycle_time,
    };
    state
        .timeline_channel
        .roundtrip(timeline_request)
        .await
        .map_err(|err| {
            error!(kind = "timeline channel roundtrip", %err);
            INTERNAL_ERROR
        })
}

#[instrument(name = "performance_api_handler", skip_all)]
async fn performance_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PerformanceQueryParams>,
) -> Result<Json<f32>, HandlerError> {
    let config_request = ConfigRequest { id: id.clone() };
    let ConfigFromApi {
        target_cycle_time, ..
    } = state
        .config_channel
        .roundtrip(config_request)
        .await
        .map_err(|err| {
            error!(kind = "config channel roundtrip", %err);
            INTERNAL_ERROR
        })?;
    let performance_request = PerformanceRequest {
        id,
        now: query.client_time,
        target_cycle_time,
    };
    state
        .performance_channel
        .roundtrip(performance_request)
        .await
        .map(Json)
        .map_err(|err| {
            error!(kind = "performance channel roundtrip", %err);
            INTERNAL_ERROR
        })
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use crate::channel::{roundtrip_channel, RoundtripSender};

    use super::*;

    fn successful_config_tx() -> RoundtripSender<ConfigRequest, ConfigFromApi> {
        let (tx, mut rx) = roundtrip_channel(1);
        tokio::spawn(async move {
            let (_, reply_tx) = rx.recv().await.expect("channel has been closed");
            let config = ConfigFromApi {
                target_cycle_time: 1.2,
                target_efficiency: 3.4,
            };
            reply_tx.send(config).expect("error sending response");
        });
        tx
    }

    mod health_handler {
        use crate::channel::roundtrip_channel;

        use super::*;

        fn testing_fixture(health_channel: HealthChannel) -> (Router, Request<Body>) {
            let (config_channel, _) = roundtrip_channel(1);
            let (timeline_channel, _) = roundtrip_channel(1);
            let (performance_channel, _) = roundtrip_channel(1);
            let app = app(AppState {
                health_channel,
                config_channel,
                timeline_channel,
                performance_channel,
            });
            let req = Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap();
            (app, req)
        }

        #[tokio::test]
        async fn roundtrip_error() {
            let (tx, _) = roundtrip_channel(1);
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn unhealthy() {
            let (tx, mut rx) = roundtrip_channel(1);
            tokio::spawn(async move {
                let (_, reply_tx) = rx.recv().await.expect("channel has been closed");
                reply_tx
                    .send(StatusCode::INTERNAL_SERVER_ERROR)
                    .expect("error sending response");
            });
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn healthy() {
            let (tx, mut rx) = roundtrip_channel(1);
            tokio::spawn(async move {
                let (_, reply_tx) = rx.recv().await.expect("channel has been closed");
                reply_tx
                    .send(StatusCode::OK)
                    .expect("error sending response");
            });
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
        }
    }

    mod timeline_handler {
        use std::vec;

        use crate::channel::{roundtrip_channel, RoundtripSender};
        use crate::model::TimelineSlot;

        use super::*;

        fn testing_fixture(
            config_channel: ConfigChannel,
            timeline_channel: TimelineChannel,
        ) -> (Router, Request<Body>) {
            let (health_channel, _) = roundtrip_channel(1);
            let (performance_channel, _) = roundtrip_channel(1);
            let app = app(AppState {
                health_channel,
                config_channel,
                timeline_channel,
                performance_channel,
            });
            let req = Request::builder()
                .uri("/timeline/someid")
                .body(Body::empty())
                .unwrap();
            (app, req)
        }

        fn successful_timeline_tx() -> RoundtripSender<TimelineRequest, TimelineResponse> {
            let (tx, mut rx) = roundtrip_channel(1);
            tokio::spawn(async move {
                let (_, reply_tx) = rx.recv().await.expect("channel has been closed");
                let slots: Vec<TimelineSlot> = vec![
                    TimelineSlot {
                        start: "1970-01-01T00:00:00Z".parse().unwrap(),
                        color: None,
                    },
                    TimelineSlot {
                        start: "1984-12-09T04:30:00Z".parse().unwrap(),
                        color: Some(5),
                    },
                ];
                reply_tx.send(slots.into()).expect("error sending response");
            });
            tx
        }

        #[tokio::test]
        async fn config_roundtrip_error() {
            let (config_tx, _) = roundtrip_channel(1);
            let timeline_tx = successful_timeline_tx();
            let (app, req) = testing_fixture(config_tx, timeline_tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn timeline_roundtrip_error() {
            let config_tx = successful_config_tx();
            let (timeline_tx, _) = roundtrip_channel(1);
            let (app, req) = testing_fixture(config_tx, timeline_tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn success() {
            let config_tx = successful_config_tx();
            let timeline_tx = successful_timeline_tx();
            let (app, req) = testing_fixture(config_tx, timeline_tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            assert_eq!(res.headers()["Content-Type"], "application/msgpack");
            let body = hyper::body::to_bytes(res).await.unwrap();
            let expected = [
                0x92, 0x92, 0x00, 0xc0, 0x92, 0xce, 0x1c, 0x19, 0x37, 0x48, 0x05,
            ];
            assert_eq!(body.to_vec(), expected);
        }
    }

    mod performance_handler {
        use super::*;

        fn testing_fixture(
            config_channel: ConfigChannel,
            performance_channel: PerformanceChannel,
        ) -> (Router, Request<Body>) {
            let (health_channel, _) = roundtrip_channel(1);
            let (timeline_channel, _) = roundtrip_channel(1);
            let app = app(AppState {
                health_channel,
                config_channel,
                timeline_channel,
                performance_channel,
            });
            let req = Request::builder()
                .uri("/performance/anid?clientTime=1984-12-09T11:30:00%2B05:00")
                .body(Body::empty())
                .unwrap();
            (app, req)
        }

        fn successful_performance_tx() -> RoundtripSender<PerformanceRequest, f32> {
            let (tx, mut rx) = roundtrip_channel(1);
            tokio::spawn(async move {
                let (_, reply_tx) = rx.recv().await.expect("channel has been closed");
                reply_tx.send(42.4242).expect("error sending response");
            });
            tx
        }

        #[tokio::test]
        async fn config_roundtrip_error() {
            let (config_tx, _) = roundtrip_channel(1);
            let performance_tx = successful_performance_tx();
            let (app, req) = testing_fixture(config_tx, performance_tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn performance_roundtrip_error() {
            let config_tx = successful_config_tx();
            let (performance_tx, _) = roundtrip_channel(1);
            let (app, req) = testing_fixture(config_tx, performance_tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn success() {
            let config_tx = successful_config_tx();
            let performance_tx = successful_performance_tx();
            let (app, req) = testing_fixture(config_tx, performance_tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            assert_eq!(res.headers()["Content-Type"], "application/json");
            let body = hyper::body::to_bytes(res).await.unwrap();
            assert_eq!(body, "42.4242");
        }
    }
}
