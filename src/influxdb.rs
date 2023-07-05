use std::io;

use arcstr::ArcStr;
use chrono::{DateTime, Utc};
use clap::Args;
use csv_async::AsyncReaderBuilder;
use futures_util::{Stream, TryStreamExt};
use reqwest::{header, Client as HttpClient, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};
use tokio::task::{spawn_blocking, JoinHandle};
use tracing::{error, info, info_span, instrument, Instrument};
use url::Url;

use crate::model::{TimelineResponse, TimelineSlot};

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

pub(crate) struct HealthRequest {
    pub(crate) response_channel: oneshot::Sender<StatusCode>,
}

pub(crate) type HealthChannel = mpsc::Sender<HealthRequest>;

pub(crate) struct TimelineRequest {
    pub(crate) id: String,
    pub(crate) response_channel: oneshot::Sender<TimelineResponse>,
}

pub(crate) type TimelineChannel = mpsc::Sender<TimelineRequest>;

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

#[derive(Clone)]
pub(crate) struct Client {
    base_url: Url,
    auth_header: ArcStr,
    org: ArcStr,
    bucket: ArcStr,
    measurement: ArcStr,
    http_client: HttpClient,
}

impl Client {
    pub(crate) fn new(config: &Config) -> Self {
        let base_url = config.influxdb_url.clone();
        let auth_header = ArcStr::from(format!("Token {}", config.influxdb_api_token));
        let org = ArcStr::from(&config.influxdb_org);
        let bucket = ArcStr::from(&config.influxdb_bucket);
        let measurement = ArcStr::from(&config.influxdb_measurement);
        let http_client = HttpClient::new();

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
    async fn query<T>(
        &self,
        flux_query: &str,
    ) -> Result<impl Stream<Item = Result<T, csv_async::Error>>, ()>
    where
        T: DeserializeOwned + 'static,
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

        let row_stream = AsyncReaderBuilder::new()
            .comment(Some(b'#'))
            .create_deserializer(reader)
            .into_deserialize::<T>();

        Ok(row_stream)
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
                    let Ok(rows_stream) = cloned_self.query::<TimelineRow>(&flux_query).await else {
                        continue;
                    };
                    let mut rows: Vec<_> = match rows_stream.try_collect().await {
                        Ok(rows) => rows,
                        Err(err) => {
                            error!(kind = "CSV data processing",%err);
                            continue;
                        }
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
                };
                let client = Client::new(&config);
                let result = client.query::<()>(FLUX_QUERY).await;
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
                let client = Client::new(&config);
                let rows = client
                    .query::<(String, u8)>(FLUX_QUERY)
                    .await
                    .unwrap()
                    .try_collect::<Vec<_>>()
                    .await
                    .unwrap();
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
                    .match_body(Matcher::Regex("r\\.id == \"someid\"".to_string()))
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
            async fn csv_error() {
                let mut server = Server::new_async().await;
                let mock = server_mock(&mut server)
                    .with_status(200)
                    .with_body("something,otherthing\n1,2")
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: Default::default(),
                    influxdb_org: Default::default(),
                    influxdb_bucket: Default::default(),
                    influxdb_measurement: Default::default(),
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
                let mut server = Server::new_async().await;
                let body = indoc! {"
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
                let mock = server_mock(&mut server)
                    .with_status(200)
                    .with_body(body)
                    .create_async()
                    .await;
                let config = Config {
                    influxdb_url: server.url().parse().unwrap(),
                    influxdb_api_token: Default::default(),
                    influxdb_org: Default::default(),
                    influxdb_bucket: Default::default(),
                    influxdb_measurement: Default::default(),
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
    }
}
