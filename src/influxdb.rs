use std::io;
use std::sync::Arc;

use arcstr::ArcStr;
use chrono::{DateTime, Duration, FixedOffset, NaiveTime, Utc};
use clap::Args;
use csv_async::AsyncReaderBuilder;
use futures_util::TryStreamExt;
use reqwest::{header, Client as HttpClient, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tokio::task::{spawn_blocking, JoinHandle};
use tracing::{error, info, info_span, instrument, Instrument};
use url::Url;

use crate::model::{TimelineResponse, TimelineSlot};
use crate::time::{determine_shift_start, excluded_duration, time_span_parser};

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

    /// Comma-separated shift start times in `%H:%M:%S` format.
    #[arg(env, long, value_delimiter = ',', required = true)]
    shift_start_times: Vec<NaiveTime>,

    /// Comma-separated pause time definitions (`%H:%M:%S/{minutes}`).
    #[arg(env, long, value_delimiter = ',', value_parser = time_span_parser)]
    pauses: Vec<(NaiveTime, NaiveTime)>,
}

pub(crate) struct HealthRequest {
    pub(crate) response_channel: oneshot::Sender<StatusCode>,
}

pub(crate) type HealthChannel = mpsc::Sender<HealthRequest>;

pub(crate) struct TimelineRequest {
    pub(crate) id: String,
    pub(crate) response_channel: oneshot::Sender<TimelineResponse>,
}

pub(crate) type TimelineChannel = mpsc::Sender<TimelineRequest>;

pub(crate) struct PerformanceRequest {
    pub(crate) id: String,
    pub(crate) now: DateTime<FixedOffset>,
    pub(crate) response_channel: oneshot::Sender<f32>,
}

pub(crate) type PerformanceChannel = mpsc::Sender<PerformanceRequest>;

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
    part_ref: String,
}

#[derive(Clone)]
pub(crate) struct Client {
    base_url: Arc<Url>,
    auth_header: ArcStr,
    org: ArcStr,
    bucket: ArcStr,
    measurement: ArcStr,
    shift_start_times: Arc<Vec<NaiveTime>>,
    pauses: Arc<Vec<(NaiveTime, NaiveTime)>>,
    http_client: HttpClient,
}

impl Client {
    pub(crate) fn new(config: &Config) -> Self {
        let base_url = Arc::new(config.influxdb_url.clone());
        let auth_header = ArcStr::from(format!("Token {}", config.influxdb_api_token));
        let org = ArcStr::from(&config.influxdb_org);
        let bucket = ArcStr::from(&config.influxdb_bucket);
        let measurement = ArcStr::from(&config.influxdb_measurement);
        let shift_start_times = Arc::new(config.shift_start_times.clone());
        let pauses = Arc::new(config.pauses.clone());
        let http_client = HttpClient::new();

        Self {
            base_url,
            auth_header,
            org,
            bucket,
            measurement,
            shift_start_times,
            pauses,
            http_client,
        }
    }

    #[instrument(skip_all, name = "influxdb_query")]
    async fn query<T>(&self, flux_query: &str) -> Result<Vec<T>, ()>
    where
        T: DeserializeOwned,
    {
        let mut url = self.base_url.join("/api/v2/query").unwrap();
        url.query_pairs_mut().append_pair("org", self.org.as_str());
        let body = flux_query
            .replace("__bucketplaceholder__", &self.bucket)
            .replace("__measurementplaceholder__", &self.measurement);

        let response = self
            .http_client
            .post(url)
            .header(header::ACCEPT, "application/csv")
            .header(header::AUTHORIZATION, self.auth_header.as_str())
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
        let (tx, mut rx) = mpsc::channel::<HealthRequest>(1);
        let cloned_self = self.clone();
        let url = self.base_url.join("/health").unwrap();

        let task = tokio::spawn(
            async move {
                info!(status = "started");

                while let Some(request) = rx.recv().await {
                    let response = match cloned_self.http_client.get(url.clone()).send().await {
                        Ok(resp) => resp,
                        Err(err) => {
                            error!(kind = "request sending", %err);
                            continue;
                        }
                    };
                    if request.response_channel.send(response.status()).is_err() {
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
        let (tx, mut rx) = mpsc::channel::<TimelineRequest>(1);
        let cloned_self = self.clone();

        let task = tokio::spawn(
            async move {
                info!(status = "started");

                while let Some(request) = rx.recv().await {
                    let flux_query = FLUX_QUERY.replace("__idplaceholder__", &request.id);
                    let Ok(mut rows) = cloned_self.query::<TimelineRow>(&flux_query).await else {
                        continue;
                    };
                    let slots: Vec<TimelineSlot> = spawn_blocking(|| {
                        let Some(last_row) = rows.pop() else {
                            return Vec::new();
                        };
                        rows.dedup_by_key(|row| row.color);
                        rows.push(last_row);
                        rows.into_iter()
                            .map(|TimelineRow { time: start, color }| TimelineSlot { start, color })
                            .collect()
                    })
                    .await
                    .unwrap();
                    if request.response_channel.send(slots.into()).is_err() {
                        error!(kind = "response channel sending");
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
        let (tx, mut rx) = mpsc::channel::<PerformanceRequest>(1);
        let cloned_self = self.clone();

        let task = tokio::spawn(
            async move {
                info!(status = "started");

                while let Some(request) = rx.recv().await {
                    let start_time =
                        determine_shift_start(request.now, &cloned_self.shift_start_times);
                    let flux_query = FLUX_QUERY
                        .replace("__idplaceholder__", &request.id)
                        .replace("__startplaceholder__", &start_time.to_rfc3339());
                    let Ok(rows) = cloned_self.query::<PerformanceRow>(&flux_query).await else {
                        continue;
                    };
                    let pauses = Arc::clone(&cloned_self.pauses);
                    let performance = spawn_blocking(move || {
                        let (expected_parts, done_parts) = rows
                            .into_iter()
                            .filter(|row| row.elapsed.is_positive() && !row.part_ref.is_empty())
                            .fold((0.0, 0), |(expected, done), row| {
                                // TODO: query cycle time for each campaign.
                                const CYCLE_TIME_SECONDS: f32 = 21.3;
                                let end =
                                    row.end.with_timezone(&request.now.timezone()).naive_local();
                                let duration = Duration::minutes(row.elapsed);
                                let start = end - duration;
                                let pause_duration = excluded_duration(start..end, &pauses);
                                let effective_duration = duration - pause_duration;
                                let expected_parts =
                                    effective_duration.num_seconds() as f32 / CYCLE_TIME_SECONDS;
                                (expected + expected_parts, done + row.good_parts)
                            });
                        f32::from(done_parts) / expected_parts * 100.0
                    })
                    .await
                    .unwrap();
                    if request.response_channel.send(performance).is_err() {
                        error!(kind = "response channel sending");
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
                    shift_start_times: Default::default(),
                    pauses: Default::default(),
                };
                let client = Client::new(&config);
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
                    shift_start_times: Default::default(),
                    pauses: Default::default(),
                };
                let client = Client::new(&config);
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
                    shift_start_times: Default::default(),
                    pauses: Default::default(),
                };
                let client = Client::new(&config);
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
                    shift_start_times: Default::default(),
                    pauses: Default::default(),
                };
                let client = Client::new(&config);
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
                    shift_start_times: Default::default(),
                    pauses: Default::default(),
                };
                let client = Client::new(&config);
                let (tx, rx) = oneshot::channel();
                let request = HealthRequest {
                    response_channel: tx,
                };
                let (health_channel, task) = client.handle_health();
                health_channel.send(request).await.unwrap();
                assert!(rx.await.is_err());
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
                    shift_start_times: Default::default(),
                    pauses: Default::default(),
                };
                let client = Client::new(&config);
                let (tx, rx) = oneshot::channel();
                let request = HealthRequest {
                    response_channel: tx,
                };
                let (health_channel, task) = client.handle_health();
                health_channel.send(request).await.unwrap();
                let status_code = rx.await.unwrap();
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
                    shift_start_times: Default::default(),
                    pauses: Default::default(),
                };
                let client = Client::new(&config);
                let (tx, rx) = oneshot::channel();
                let request = HealthRequest {
                    response_channel: tx,
                };
                let (health_channel, task) = client.handle_health();
                health_channel.send(request).await.unwrap();
                let status_code = rx.await.unwrap();
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
                    .match_body(Matcher::Regex(r#"r\.id == "someid""#.to_string()))
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
                    shift_start_times: Default::default(),
                    pauses: Default::default(),
                };
                let client = Client::new(&config);
                let (tx, rx) = oneshot::channel();
                let request = TimelineRequest {
                    id: "someid".to_string(),
                    response_channel: tx,
                };
                let (timeline_channel, task) = client.handle_timeline();
                timeline_channel.send(request).await.unwrap();
                assert!(rx.await.is_err());
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
                    shift_start_times: Default::default(),
                    pauses: Default::default(),
                };
                let client = Client::new(&config);
                let (tx, rx) = oneshot::channel();
                let request = TimelineRequest {
                    id: "someid".to_string(),
                    response_channel: tx,
                };
                let (timeline_channel, task) = client.handle_timeline();
                timeline_channel.send(request).await.unwrap();
                let slots = rx.await.unwrap();
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
                    shift_start_times: Default::default(),
                    pauses: Default::default(),
                };
                let client = Client::new(&config);
                let (tx, rx) = oneshot::channel();
                let request = TimelineRequest {
                    id: "someid".to_string(),
                    response_channel: tx,
                };
                let (timeline_channel, task) = client.handle_timeline();
                timeline_channel.send(request).await.unwrap();
                let slots = rx.await.unwrap();
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
            use indoc::indoc;

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
                    shift_start_times: shift_start_times(),
                    pauses: pauses(),
                };
                let client = Client::new(&config);
                let (tx, rx) = oneshot::channel();
                let request = PerformanceRequest {
                    id: "otherid".to_string(),
                    now: "1984-12-09T04:30:00+02:00".parse().unwrap(),
                    response_channel: tx,
                };
                let (performance_channel, task) = client.handle_performance();
                performance_channel.send(request).await.unwrap();
                assert!(rx.await.is_err());
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
                    shift_start_times: shift_start_times(),
                    pauses: pauses(),
                };
                let client = Client::new(&config);
                let (tx, rx) = oneshot::channel();
                let request = PerformanceRequest {
                    id: "otherid".to_string(),
                    now: "1984-12-09T04:30:00+02:00".parse().unwrap(),
                    response_channel: tx,
                };
                let (performance_channel, task) = client.handle_performance();
                performance_channel.send(request).await.unwrap();
                let performance_ratio = rx.await.unwrap();
                assert!(performance_ratio.is_nan());
                mock.assert_async().await;
                assert!(!task.is_finished());
            }

            #[tokio::test]
            async fn success() {
                const BODY: &str = indoc! {"
                    elapsed,end,goodParts,partRef
                    -1,1984-12-09T00:00:00+02:00,500,invalid
                    60,1984-12-09T00:00:00+02:00,500,
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
                    shift_start_times: shift_start_times(),
                    pauses: pauses(),
                };
                let client = Client::new(&config);
                let (tx, rx) = oneshot::channel();
                let request = PerformanceRequest {
                    id: "otherid".to_string(),
                    now: "1984-12-09T04:30:00+02:00".parse().unwrap(),
                    response_channel: tx,
                };
                let (performance_channel, task) = client.handle_performance();
                performance_channel.send(request).await.unwrap();
                let performance_ratio = rx.await.unwrap();
                assert!(60.2 < performance_ratio && performance_ratio < 60.3);
                mock.assert_async().await;
                assert!(!task.is_finished());
            }
        }
    }
}
