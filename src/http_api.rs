use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::HeaderValue;
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use bytes::{BufMut, BytesMut};
use chrono::{DateTime, FixedOffset};
use reqwest::{header, StatusCode};
use serde::Deserialize;
use tokio::sync::oneshot;
use tracing::{error, instrument};

use crate::influxdb::{
    HealthChannel, HealthRequest, PerformanceChannel, PerformanceRequest, TimelineChannel,
    TimelineRequest,
};
use crate::model::TimelineResponse;

const CHANNEL_SEND_TIMEOUT: Duration = Duration::from_millis(100);

const INTERNAL_ERROR: (StatusCode, &str) =
    (StatusCode::INTERNAL_SERVER_ERROR, "internal server error");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimelineQueryParams {
    target_cycle_time: f32,
}

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
    target_cycle_time: f32,
}

#[derive(Clone)]
struct AppState {
    health_channel: HealthChannel,
    timeline_channel: TimelineChannel,
    performance_channel: PerformanceChannel,
}

pub(crate) fn app(
    health_channel: HealthChannel,
    timeline_channel: TimelineChannel,
    performance_channel: PerformanceChannel,
) -> Router {
    Router::new()
        .route("/health", routing::get(health_handler))
        .route("/timeline/:id", routing::get(timeline_handler))
        .route("/performance/:id", routing::get(performance_handler))
        .with_state(AppState {
            health_channel,
            timeline_channel,
            performance_channel,
        })
}

#[instrument(name = "health_api_handler", skip_all)]
async fn health_handler(State(state): State<AppState>) -> Result<StatusCode, impl IntoResponse> {
    let (response_channel, rx) = oneshot::channel();
    let request = HealthRequest { response_channel };
    state
        .health_channel
        .send_timeout(request, CHANNEL_SEND_TIMEOUT)
        .await
        .map_err(|err| {
            error!(kind = "request channel sending", %err);
            INTERNAL_ERROR
        })?;
    rx.await.map_err(|err| {
        error!(kind = "response channel receiving", %err);
        INTERNAL_ERROR
    })
}

#[instrument(name = "timeline_api_handler", skip_all)]
async fn timeline_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<TimelineQueryParams>,
) -> Result<TimelineResponse, impl IntoResponse> {
    let (response_channel, rx) = oneshot::channel();
    let request = TimelineRequest {
        id,
        target_cycle_time: query.target_cycle_time,
        response_channel,
    };
    state
        .timeline_channel
        .send_timeout(request, CHANNEL_SEND_TIMEOUT)
        .await
        .map_err(|err| {
            error!(kind="request channel sending", %err);
            INTERNAL_ERROR
        })?;
    rx.await.map_err(|err| {
        error!(kind="response channel receiving", %err);
        INTERNAL_ERROR
    })
}

#[instrument(name = "performance_api_handler", skip_all)]
async fn performance_handler(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PerformanceQueryParams>,
) -> Result<Json<f32>, impl IntoResponse> {
    let (response_channel, rx) = oneshot::channel();
    let request = PerformanceRequest {
        id,
        now: query.client_time,
        target_cycle_time: query.target_cycle_time,
        response_channel,
    };
    state
        .performance_channel
        .send_timeout(request, CHANNEL_SEND_TIMEOUT)
        .await
        .map_err(|err| {
            error!(kind="request channel sending", %err);
            INTERNAL_ERROR
        })?;
    rx.await.map(Json).map_err(|err| {
        error!(kind="response channel receiving", %err);
        INTERNAL_ERROR
    })
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use tokio::sync::mpsc;
    use tower::ServiceExt;

    use super::*;

    mod health_handler {
        use super::*;

        fn testing_fixture(health_channel: HealthChannel) -> (Router, Request<Body>) {
            let (timeline_channel, _) = mpsc::channel(1);
            let (performance_channel, _) = mpsc::channel(1);
            let app = app(health_channel, timeline_channel, performance_channel);
            let req = Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap();
            (app, req)
        }

        #[tokio::test]
        async fn request_sending_error() {
            let (tx, _) = mpsc::channel(1);
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn request_sending_timeout() {
            let (tx, _rx) = mpsc::channel(1);
            tx.send({
                let (response_channel, _) = oneshot::channel();
                HealthRequest { response_channel }
            })
            .await
            .unwrap();
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn outcome_channel_receiving_error() {
            let (tx, mut rx) = mpsc::channel(1);
            tokio::spawn(async move {
                // Consume and drop the response channel
                let _ = rx.recv().await.expect("channel has been closed");
            });
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn unhealthy() {
            let (tx, mut rx) = mpsc::channel::<HealthRequest>(1);
            tokio::spawn(async move {
                let request_tx = rx.recv().await.expect("channel has been closed");
                request_tx
                    .response_channel
                    .send(StatusCode::INTERNAL_SERVER_ERROR)
                    .expect("error sending response");
            });
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn healthy() {
            let (tx, mut rx) = mpsc::channel::<HealthRequest>(1);
            tokio::spawn(async move {
                let request_tx = rx.recv().await.expect("channel has been closed");
                request_tx
                    .response_channel
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

        use crate::model::TimelineSlot;

        use super::*;

        fn testing_fixture(timeline_channel: TimelineChannel) -> (Router, Request<Body>) {
            let (health_channel, _) = mpsc::channel(1);
            let (performance_channel, _) = mpsc::channel(1);
            let app = app(health_channel, timeline_channel, performance_channel);
            let req = Request::builder()
                .uri("/timeline/someid?targetCycleTime=1.2")
                .body(Body::empty())
                .unwrap();
            (app, req)
        }

        #[tokio::test]
        async fn request_sending_error() {
            let (tx, _) = mpsc::channel(1);
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn invalid_query_params() {
            let (tx, _) = mpsc::channel(1);
            let (app, mut req) = testing_fixture(tx);
            *req.uri_mut() = "/timeline/someid?targetCycleTime=a".try_into().unwrap();
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        }

        #[tokio::test]
        async fn request_sending_timeout() {
            let (tx, _rx) = mpsc::channel(1);
            tx.send({
                let (response_channel, _) = oneshot::channel();
                TimelineRequest {
                    id: Default::default(),
                    target_cycle_time: Default::default(),
                    response_channel,
                }
            })
            .await
            .unwrap();
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn outcome_channel_receiving_error() {
            let (tx, mut rx) = mpsc::channel(1);
            tokio::spawn(async move {
                // Consume and drop the response channel
                let _ = rx.recv().await.expect("channel has been closed");
            });
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn success() {
            let (tx, mut rx) = mpsc::channel::<TimelineRequest>(1);
            tokio::spawn(async move {
                let request_tx = rx.recv().await.expect("channel has been closed");
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
                request_tx
                    .response_channel
                    .send(slots.into())
                    .expect("error sending response");
            });
            let (app, req) = testing_fixture(tx);
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

        fn testing_fixture(performance_channel: PerformanceChannel) -> (Router, Request<Body>) {
            let (health_channel, _) = mpsc::channel(1);
            let (timeline_channel, _) = mpsc::channel(1);
            let app = app(health_channel, timeline_channel, performance_channel);
            let req = Request::builder()
                .uri(
                    "/performance/anid?clientTime=1984-12-09T11:30:00%2B05:00&targetCycleTime=12.3",
                )
                .body(Body::empty())
                .unwrap();
            (app, req)
        }

        #[tokio::test]
        async fn invalid_query_params() {
            let (tx, _) = mpsc::channel(1);
            let (app, mut req) = testing_fixture(tx);
            *req.uri_mut() = "/performance/anid?clientTime=a".try_into().unwrap();
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        }

        #[tokio::test]
        async fn request_sending_error() {
            let (tx, _) = mpsc::channel(1);
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn request_sending_timeout() {
            let (tx, _rx) = mpsc::channel(1);
            tx.send({
                let (response_channel, _) = oneshot::channel();
                PerformanceRequest {
                    id: Default::default(),
                    now: Default::default(),
                    target_cycle_time: Default::default(),
                    response_channel,
                }
            })
            .await
            .unwrap();
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn outcome_channel_receiving_error() {
            let (tx, mut rx) = mpsc::channel(1);
            tokio::spawn(async move {
                // Consume and drop the response channel
                let _ = rx.recv().await.expect("channel has been closed");
            });
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn success() {
            let (tx, mut rx) = mpsc::channel::<PerformanceRequest>(1);
            tokio::spawn(async move {
                let request_tx = rx.recv().await.expect("channel has been closed");
                request_tx
                    .response_channel
                    .send(42.4242)
                    .expect("error sending response");
            });
            let (app, req) = testing_fixture(tx);
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            assert_eq!(res.headers()["Content-Type"], "application/json");
            let body = hyper::body::to_bytes(res).await.unwrap();
            assert_eq!(body, "42.4242");
        }
    }
}
