use std::io;
use std::sync::Arc;

use chrono::serde::ts_seconds;
use chrono::{DateTime, Duration, NaiveTime, Utc};
use chrono_tz::Tz;
use clap::Args;
use csv_async::AsyncReaderBuilder;
use futures_util::TryStreamExt;
use reqwest::{header, Client as HttpClient, StatusCode};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tracing::{error, info, info_span, instrument, Instrument};
use url::Url;

use crate::channel::{roundtrip_channel, RoundtripSender};
use crate::time::{apply_time_spans, find_shift_bounds};

#[derive(Args)]
#[group(skip)]
pub(crate) struct Config {
    /// InfluxDB base URL
    #[arg(env, long, default_value = "http://influxdb:8086")]
    influxdb_url: Url,

    /// InfluxDB API token with read permission on configured bucket
    #[arg(env, long)]
    influxdb_api_token: String,

    /// InfluxDB organization name or ID
    #[arg(env, long)]
    influxdb_org: String,

    /// InfluxDB bucket
    #[arg(env, long)]
    influxdb_bucket: String,

    /// InfluxDB measurement
    #[arg(env, long)]
    influxdb_measurement: String,
}

pub(crate) type HealthChannel = RoundtripSender<(), StatusCode>;

pub(crate) struct TimelineRequest {
    pub(crate) id: String,
    pub(crate) target_cycle_time: f32,
}

#[derive(Debug, PartialEq, Serialize)]
pub(crate) struct TimelineSlot {
    #[serde(with = "ts_seconds")]
    pub(crate) start: DateTime<Utc>,
    pub(crate) color: Option<u8>,
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

pub(crate) type TimelineChannel = RoundtripSender<TimelineRequest, TimelineResponse>;

pub(crate) struct PerformanceRequest {
    pub(crate) id: String,
    pub(crate) shift_start_times: Vec<NaiveTime>,
    pub(crate) pauses: Vec<(NaiveTime, NaiveTime)>,
    pub(crate) timezone: Tz,
    pub(crate) target_cycle_time: f32,
}

pub(crate) type PerformanceChannel = RoundtripSender<PerformanceRequest, f32>;

#[derive(Deserialize)]
struct QueryResponse {
    message: String,
}

#[derive(Deserialize)]
struct TimelineRow {
    #[serde(rename = "_time")]
    time: DateTime<Utc>,
    color: Option<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PerformanceRow {
    /// Number of elapsed minutes.
    elapsed: i64,
    /// End timestamp.
    end: DateTime<Utc>,
    /// Good parts counter.
    good_parts: u16,
    /// Part reference.
    _part_ref: String,
}

#[derive(Clone)]
pub(crate) struct Client {
    base_url: Arc<Url>,
    auth_header: Arc<str>,
    org: Arc<str>,
    bucket: Arc<str>,
    measurement: Arc<str>,
    http_client: HttpClient,
}

impl Client {
    pub(crate) fn new(config: &Config, http_client: HttpClient) -> Self {
        let base_url = Arc::new(config.influxdb_url.clone());
        let auth_header = Arc::from(format!("Token {}", config.influxdb_api_token).as_str());
        let org = Arc::from(config.influxdb_org.as_str());
        let bucket = Arc::from(config.influxdb_bucket.as_str());
        let measurement = Arc::from(config.influxdb_measurement.as_str());

        Self {
            base_url,
            auth_header,
            org,
            bucket,
            measurement,
            http_client,
        }
    }

    #[instrument(skip_all, name = "influxdb_query")]
    async fn query<T>(&self, flux_query: &str) -> Result<Vec<T>, ()>
    where
        T: DeserializeOwned,
    {
        let mut url = self.base_url.join("/api/v2/query").unwrap();
        url.query_pairs_mut().append_pair("org", self.org.as_ref());
        let body = flux_query
            .replace("__bucketplaceholder__", &self.bucket)
            .replace("__measurementplaceholder__", &self.measurement);

        let response = self
            .http_client
            .post(url)
            .header(header::ACCEPT, "application/csv")
            .header(header::AUTHORIZATION, self.auth_header.as_ref())
            .header(header::CONTENT_TYPE, "application/vnd.flux")
            .body(body)
            .send()
            .await
            .map_err(|err| {
                error!(kind = "request sending", %err);
            })?;

        let status_code = response.status();
        if !status_code.is_success() {
            let message = response
                .json()
                .await
                .map(|QueryResponse { message }| message)
                .unwrap_or_default();
            error!(kind = "response status", %status_code, message);
            return Err(());
        }

        let reader = response
            .bytes_stream()
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
            .into_async_read();

        AsyncReaderBuilder::new()
            .comment(Some(b'#'))
            .create_deserializer(reader)
            .into_deserialize::<T>()
            .try_collect()
            .await
            .map_err(|err| {
                error!(kind="CSV data processing",%err);
            })
    }

    pub(crate) fn handle_health(&self) -> (HealthChannel, JoinHandle<()>) {
        let (tx, mut rx) = roundtrip_channel(10);
        let http_client = self.http_client.clone();
        let url = self.base_url.join("/health").unwrap();

        let task = tokio::spawn(
            async move {
                info!(status = "started");

                while let Some((_, _, reply_tx)) = rx.recv().await {
                    let response = match http_client.get(url.clone()).send().await {
                        Ok(resp) => resp,
                        Err(err) => {
                            error!(kind = "request sending", %err);
                            continue;
                        }
                    };
                    if reply_tx.send(response.status()).is_err() {
                        error!(kind = "response channel sending");
                    }
                }

                info!(status = "terminating");
            }
            .instrument(info_span!("influxdb_health_handler")),
        );

        (tx, task)
    }

    pub(crate) fn handle_timeline(&self) -> (TimelineChannel, JoinHandle<()>) {
        const FLUX_QUERY: &str = include_str!("timeline.flux");
        let (tx, mut rx) = roundtrip_channel::<TimelineRequest, TimelineResponse>(10);
        let cloned_self = self.clone();

        let task = tokio::spawn(
            async move {
                info!(status = "started");

                while let Some((request, cancellation_token, reply_tx)) = rx.recv().await {
                    let inner_task = async {
                        let flux_query = FLUX_QUERY
                            .replace("__idplaceholder__", &request.id)
                            .replace(
                                "__targetcycletimeplaceholder__",
                                &request.target_cycle_time.to_string(),
                            );
                        let Ok(mut rows) = cloned_self.query::<TimelineRow>(&flux_query).await
                        else {
                            return;
                        };
                        if let Some(last_row) = rows.pop() {
                            rows.dedup_by_key(|row| row.color);
                            rows.push(last_row);
                        };
                        let slots = rows
                            .into_iter()
                            .map(|TimelineRow { time: start, color }| TimelineSlot { start, color })
                            .collect::<Vec<_>>();
                        if reply_tx.send(slots.into()).is_err() {
                            error!(kind = "response channel sending");
                        }
                    };
                    tokio::select! {
                        _ = cancellation_token.cancelled() => {
                            info!(msg="request was cancelled");
                        },
                        _ = inner_task => {},
                    }
                }

                info!(status = "terminating");
            }
            .instrument(info_span!("influxdb_timeline_handler")),
        );

        (tx, task)
    }

    pub(crate) fn handle_performance(&self) -> (PerformanceChannel, JoinHandle<()>) {
        const FLUX_QUERY: &str = include_str!("performance.flux");
        let (tx, mut rx) = roundtrip_channel::<PerformanceRequest, f32>(10);
        let cloned_self = self.clone();

        let task = tokio::spawn(
            async move {
                info!(status = "started");

                while let Some((request, cancellation_token, reply_tx)) = rx.recv().await {
                    let inner_task = async {
                        let (start_time, _) =
                            find_shift_bounds(&request.timezone, &request.shift_start_times);
                        let flux_query = FLUX_QUERY
                            .replace("__idplaceholder__", &request.id)
                            .replace("__startplaceholder__", &start_time.to_rfc3339());
                        let Ok(rows) = cloned_self.query::<PerformanceRow>(&flux_query).await
                        else {
                            return;
                        };
                        let (expected_parts, done_parts) = rows
                            .into_iter()
                            .filter(|row| row.elapsed.is_positive())
                            .fold((0.0, 0), |(expected, done), row| {
                                let end = row.end.with_timezone(&request.timezone).naive_local();
                                let duration = Duration::minutes(row.elapsed);
                                let start = end - duration;
                                let pause_duration = apply_time_spans(start..end, &request.pauses)
                                    .into_iter()
                                    .fold(Duration::zero(), |acc, (span_start, span_end)| {
                                        acc + (span_end - span_start)
                                    });
                                let effective_duration = duration - pause_duration;
                                let effective_seconds = effective_duration.num_seconds() as f32;
                                let expected_parts = effective_seconds / request.target_cycle_time;
                                (expected + expected_parts, done + row.good_parts)
                            });
                        let performance = f32::from(done_parts) / expected_parts * 100.0;
                        if reply_tx.send(performance).is_err() {
                            error!(kind = "response channel sending");
                        }
                    };
                    tokio::select! {
                        _ = cancellation_token.cancelled() => {
                            info!(msg="request was cancelled");
                        },
                        _ = inner_task => {},
                    }
                }

                info!(status = "terminating");
            }
            .instrument(info_span!("influxdb_performance_handler")),
        );

        (tx, task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod client {
        use mockito::{Matcher, Mock, Server};

        use super::*;

        mod query {
            use super::*;

            const FLUX_QUERY: &str =
                "some Flux query with __bucketplaceholder__ and __measurementplaceholder__";

            fn server_mock(server: &mut Server) -> Mock {
                server
                    .mock("POST", "/api/v2/query")
                    .match_query(Matcher::UrlEncoded("org".into(), "someorg".into()))
                    .match_header("Accept", "application/csv")
                    .match_header("Accept-Encoding", "gzip")
                    .match_header("Authorization", "Token sometoken")
                    .match_header("Content-Type", "application/vnd.flux")
                    .match_body("some Flux query with somebucket and somemeasurement")
            }

            #[tokio::test]
            async fn request_send_failure() {
                let config = Config {
                    influxdb_url: "ftp://example.com".parse().unwrap(),
                    influxdb_api_token: "sometoken".to_string(),
                    influxdb_org: "someorg".to_string(),
                    influxdb_bucket: "somebucket".to_string(),
                    influxdb_measurement: "somemeasurement".to_string(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let result = client.query::<()>(FLUX_QUERY).await;
                assert!(result.is_err());
            }

            #[tokio::test]
            async fn bad_status_code() {
                let mut server = Server::new_async().await;
                let mock = server_mock(&mut server)
                    .with_status(500)
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: "sometoken".to_string(),
                    influxdb_org: "someorg".to_string(),
                    influxdb_bucket: "somebucket".to_string(),
                    influxdb_measurement: "somemeasurement".to_string(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let result = client.query::<()>(FLUX_QUERY).await;
                mock.assert_async().await;
                assert!(result.is_err());
            }

            #[tokio::test]
            async fn csv_error() {
                let mut server = Server::new_async().await;
                let mock = server_mock(&mut server)
                    .with_status(200)
                    .with_body("first_member,second_member\none,1\n2,two")
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: "sometoken".to_string(),
                    influxdb_org: "someorg".to_string(),
                    influxdb_bucket: "somebucket".to_string(),
                    influxdb_measurement: "somemeasurement".to_string(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let result = client.query::<(String, u8)>(FLUX_QUERY).await;
                mock.assert_async().await;
                assert!(result.is_err());
            }

            #[tokio::test]
            async fn success() {
                let mut server = Server::new_async().await;
                let mock = server_mock(&mut server)
                    .with_status(200)
                    .with_body("first_member,second_member\none,1\ntwo,2")
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: "sometoken".to_string(),
                    influxdb_org: "someorg".to_string(),
                    influxdb_bucket: "somebucket".to_string(),
                    influxdb_measurement: "somemeasurement".to_string(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let rows = client.query::<(String, u8)>(FLUX_QUERY).await.unwrap();
                mock.assert_async().await;
                assert_eq!(rows, [("one".to_string(), 1), ("two".to_string(), 2)]);
            }
        }

        mod handle_health {
            use super::*;

            #[tokio::test]
            async fn request_send_failure() {
                let config = Config {
                    influxdb_url: "ftp://example.com".parse().unwrap(),
                    influxdb_api_token: "sometoken".to_string(),
                    influxdb_org: "someorg".to_string(),
                    influxdb_bucket: "somebucket".to_string(),
                    influxdb_measurement: "somemeasurement".to_string(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let (health_channel, task) = client.handle_health();
                assert!(health_channel.roundtrip(()).await.is_err());
                assert!(!task.is_finished());
            }

            #[tokio::test]
            async fn unhealthy() {
                let mut server = Server::new_async().await;
                let mock = server
                    .mock("GET", "/health")
                    .with_status(503)
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: Default::default(),
                    influxdb_org: Default::default(),
                    influxdb_bucket: Default::default(),
                    influxdb_measurement: Default::default(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let (health_channel, task) = client.handle_health();
                let status_code = health_channel.roundtrip(()).await.unwrap();
                assert_eq!(status_code, 503);
                mock.assert_async().await;
                assert!(!task.is_finished());
            }

            #[tokio::test]
            async fn healthy() {
                let mut server = Server::new_async().await;
                let mock = server
                    .mock("GET", "/health")
                    .with_status(200)
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: Default::default(),
                    influxdb_org: Default::default(),
                    influxdb_bucket: Default::default(),
                    influxdb_measurement: Default::default(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let (health_channel, task) = client.handle_health();
                let status_code = health_channel.roundtrip(()).await.unwrap();
                assert_eq!(status_code, 200);
                mock.assert_async().await;
                assert!(!task.is_finished());
            }
        }

        mod handle_timeline {
            use indoc::indoc;

            use super::*;

            fn server_mock(server: &mut Server) -> Mock {
                server
                    .mock("POST", "/api/v2/query")
                    .match_query(Matcher::UrlEncoded("org".into(), "".into()))
                    .match_body(Matcher::AllOf(vec![
                        Matcher::Regex(r"stoppedTime = 1\.2 \*".to_string()),
                        Matcher::Regex(r#"r\.id == "someid""#.to_string()),
                    ]))
            }

            #[tokio::test]
            async fn query_error() {
                let mut server = Server::new_async().await;
                let mock = server_mock(&mut server)
                    .with_status(500)
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: Default::default(),
                    influxdb_org: Default::default(),
                    influxdb_bucket: Default::default(),
                    influxdb_measurement: Default::default(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let request = TimelineRequest {
                    id: "someid".to_string(),
                    target_cycle_time: 1.2,
                };
                let (timeline_channel, task) = client.handle_timeline();
                assert!(timeline_channel.roundtrip(request).await.is_err());
                mock.assert_async().await;
                assert!(!task.is_finished());
            }

            #[tokio::test]
            async fn success_empty() {
                let mut server = Server::new_async().await;
                let mock = server_mock(&mut server)
                    .with_status(200)
                    .with_body("")
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: Default::default(),
                    influxdb_org: Default::default(),
                    influxdb_bucket: Default::default(),
                    influxdb_measurement: Default::default(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let request = TimelineRequest {
                    id: "someid".to_string(),
                    target_cycle_time: 1.2,
                };
                let (timeline_channel, task) = client.handle_timeline();
                let slots = timeline_channel.roundtrip(request).await.unwrap();
                assert_eq!(slots.into_inner(), vec![]);
                mock.assert_async().await;
                assert!(!task.is_finished());
            }

            #[tokio::test]
            async fn success() {
                const BODY: &str = indoc! {"
                    _time,color
                    1984-12-09T04:30:00Z,1
                    1984-12-09T04:35:00Z,1
                    1984-12-09T04:40:00Z,1
                    1984-12-09T05:00:00Z,
                    1984-12-09T05:15:00Z,
                    1984-12-09T05:30:00Z,0
                    1984-12-09T05:35:00Z,0
                    1984-12-09T05:40:00Z,0
                    1984-12-09T05:45:00Z,0
                "};
                let mut server = Server::new_async().await;
                let mock = server_mock(&mut server)
                    .with_status(200)
                    .with_body(BODY)
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: Default::default(),
                    influxdb_org: Default::default(),
                    influxdb_bucket: Default::default(),
                    influxdb_measurement: Default::default(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let request = TimelineRequest {
                    id: "someid".to_string(),
                    target_cycle_time: 1.2,
                };
                let (timeline_channel, task) = client.handle_timeline();
                let slots = timeline_channel.roundtrip(request).await.unwrap();
                assert_eq!(
                    slots.into_inner(),
                    [
                        TimelineSlot {
                            start: "1984-12-09T04:30:00Z".parse().unwrap(),
                            color: Some(1)
                        },
                        TimelineSlot {
                            start: "1984-12-09T05:00:00Z".parse().unwrap(),
                            color: None
                        },
                        TimelineSlot {
                            start: "1984-12-09T05:30:00Z".parse().unwrap(),
                            color: Some(0)
                        },
                        TimelineSlot {
                            start: "1984-12-09T05:45:00Z".parse().unwrap(),
                            color: Some(0)
                        },
                    ]
                );
                mock.assert_async().await;
                assert!(!task.is_finished());
            }
        }

        mod handle_performance {
            use chrono_tz::Etc::GMTMinus2;
            use indoc::indoc;

            use crate::time::override_now;

            use super::*;

            fn server_mock(server: &mut Server) -> Mock {
                server
                    .mock("POST", "/api/v2/query")
                    .match_query(Matcher::UrlEncoded("org".into(), "".into()))
                    .match_body(Matcher::AllOf(vec![
                        Matcher::Regex(r#"r\.id == "otherid""#.to_string()),
                        Matcher::Regex(r"range\(start: 1984-12-09T00:00:00\+02:00".to_string()),
                    ]))
            }

            fn shift_start_times() -> Vec<NaiveTime> {
                vec!["00:00:00".parse().unwrap(), "12:00:00".parse().unwrap()]
            }

            fn pauses() -> Vec<(NaiveTime, NaiveTime)> {
                vec![
                    ("08:00:00".parse().unwrap(), "08:30:00".parse().unwrap()),
                    ("15:00:00".parse().unwrap(), "15:30:00".parse().unwrap()),
                ]
            }

            #[tokio::test]
            async fn query_error() {
                override_now(Some("1984-12-09T02:30:00Z".parse().unwrap()));
                let mut server = Server::new_async().await;
                let mock = server_mock(&mut server)
                    .with_status(500)
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: Default::default(),
                    influxdb_org: Default::default(),
                    influxdb_bucket: Default::default(),
                    influxdb_measurement: Default::default(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let request = PerformanceRequest {
                    id: "otherid".to_string(),
                    shift_start_times: shift_start_times(),
                    pauses: pauses(),
                    timezone: GMTMinus2,
                    target_cycle_time: 21.3,
                };
                let (performance_channel, task) = client.handle_performance();
                assert!(performance_channel.roundtrip(request).await.is_err());
                mock.assert_async().await;
                assert!(!task.is_finished());
            }

            #[tokio::test]
            async fn success_empty() {
                override_now(Some("1984-12-09T02:30:00Z".parse().unwrap()));
                let mut server = Server::new_async().await;
                let mock = server_mock(&mut server)
                    .with_status(200)
                    .with_body("")
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: Default::default(),
                    influxdb_org: Default::default(),
                    influxdb_bucket: Default::default(),
                    influxdb_measurement: Default::default(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let request = PerformanceRequest {
                    id: "otherid".to_string(),
                    shift_start_times: shift_start_times(),
                    pauses: pauses(),
                    timezone: GMTMinus2,
                    target_cycle_time: 21.3,
                };
                let (performance_channel, task) = client.handle_performance();
                let performance_ratio = performance_channel.roundtrip(request).await.unwrap();
                assert!(performance_ratio.is_nan());
                mock.assert_async().await;
                assert!(!task.is_finished());
            }

            #[tokio::test]
            async fn success() {
                override_now(Some("1984-12-09T02:30:00Z".parse().unwrap()));
                const BODY: &str = indoc! {"
                    elapsed,end,goodParts,partRef
                    -1,1984-12-09T00:00:00+02:00,500,invalid
                    60,1984-12-09T01:00:00+02:00,100,
                    30,1984-12-09T08:00:00+02:00,60,ref1
                    120,1984-12-09T10:00:00+02:00,200,ref2
                    240,1984-12-09T15:30:00+02:00,300,ref3
                "};
                let mut server = Server::new_async().await;
                let mock = server_mock(&mut server)
                    .with_status(200)
                    .with_body(BODY)
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: Default::default(),
                    influxdb_org: Default::default(),
                    influxdb_bucket: Default::default(),
                    influxdb_measurement: Default::default(),
                };
                let http_client = HttpClient::new();
                let client = Client::new(&config, http_client);
                let request = PerformanceRequest {
                    id: "otherid".to_string(),
                    shift_start_times: shift_start_times(),
                    pauses: pauses(),
                    timezone: GMTMinus2,
                    target_cycle_time: 21.3,
                };
                let (performance_channel, task) = client.handle_performance();
                let performance_ratio = performance_channel.roundtrip(request).await.unwrap();
                assert!(60.0 < performance_ratio && performance_ratio < 60.1);
                mock.assert_async().await;
                assert!(!task.is_finished());
            }
        }
    }
}
