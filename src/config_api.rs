use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{NaiveTime, Weekday};
use clap::Args;
use reqwest::{header, Client as HttpClient};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, info_span, instrument, Instrument};
use url::Url;

use crate::channel::{roundtrip_channel, RoundtripSender};

const COMMON_CONFIG_PATH: &str = "common";

#[derive(Clone)]
struct Cache<T> {
    inner: Arc<Mutex<Option<(Instant, T)>>>,
    expiration: Duration,
}

#[derive(Args)]
#[group(skip)]
pub(crate) struct Config {
    /// Configuration API URL
    #[arg(env, long)]
    config_api_url: Url,

    /// Expiration time for common configuration cache
    #[arg(env, long, default_value = "1m")]
    common_config_cache_expiration: humantime::Duration,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WeekStart {
    pub(crate) day: Weekday,
    pub(crate) shift_index: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommonConfig {
    pub(crate) shift_start_times: Vec<NaiveTime>,
    pub(crate) pauses: Vec<(NaiveTime, NaiveTime)>,
    pub(crate) week_start: WeekStart,
}

pub(crate) type CommonConfigChannel = RoundtripSender<(), CommonConfig>;

pub(crate) struct PartnerConfigRequest {
    pub(crate) id: String,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PartnerConfig {
    pub(crate) target_cycle_time: f32,
    pub(crate) target_efficiency: f32,
    pub(crate) shift_engaged: Vec<bool>,
}

#[derive(Clone)]
pub(crate) struct Client {
    config_url: Arc<Url>,
    http_client: HttpClient,
    common_config_cache: Cache<CommonConfig>,
}

pub(crate) type PartnerConfigChannel = RoundtripSender<PartnerConfigRequest, PartnerConfig>;

impl Client {
    pub(crate) fn new(config: &Config, http_client: HttpClient) -> Self {
        let config_url = Arc::new(config.config_api_url.clone());
        let common_config_cache = Cache {
            inner: Default::default(),
            expiration: config.common_config_cache_expiration.into(),
        };

        Self {
            config_url,
            http_client,
            common_config_cache,
        }
    }

    #[instrument(skip(self))]
    async fn query<T: DeserializeOwned>(&self, id: Option<&str>) -> Result<T, ()> {
        let to_join = id.unwrap_or(COMMON_CONFIG_PATH);
        let url = self.config_url.join(to_join).map_err(|err| {
            error!(kind = "joining config API URL and ID", %err);
        })?;
        let http_response = self
            .http_client
            .get(url)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|err| {
                error!(kind = "http request sending", %err);
            })?;
        let status_code = http_response.status();
        if !status_code.is_success() {
            error!(kind = "bad response status", %status_code);
            return Err(());
        }
        http_response.json().await.map_err(|err| {
            error!(kind = "response deserialization",%err);
        })
    }

    #[instrument(skip(self))]
    async fn cached_common_config(&self) -> Result<CommonConfig, ()> {
        let mut cached = self.common_config_cache.inner.lock().await;
        if let Some((cached_at, common_config)) = cached.as_ref() {
            let elapsed = cached_at.elapsed();
            if elapsed < self.common_config_cache.expiration {
                return Ok(common_config.clone());
            } else {
                let elapsed = humantime::Duration::from(elapsed);
                debug!(msg = "cache expired", %elapsed);
            }
        } else {
            debug!(msg = "empty cache");
        }
        let common_config = self.query::<CommonConfig>(None).await?;
        if !common_config
            .shift_start_times
            .windows(2)
            .all(|pair| pair[0] <= pair[1])
        {
            error!(kind = "shift start times are not sorted");
            return Err(());
        }
        if common_config.week_start.shift_index > (common_config.shift_start_times.len() - 1) {
            error!(kind = "week start shift index is out of bounds");
            return Err(());
        }
        cached.replace((Instant::now(), common_config.clone()));
        Ok(common_config)
    }

    pub(crate) fn handle_common_config(&self) -> (CommonConfigChannel, JoinHandle<()>) {
        let (tx, mut rx) = roundtrip_channel::<(), CommonConfig>(10);
        let cloned_self = self.clone();

        let task = tokio::spawn(
            async move {
                info!(status = "started");

                while let Some((_, _, reply_tx)) = rx.recv().await {
                    let Ok(common_config) = cloned_self.cached_common_config().await else {
                        continue;
                    };
                    if reply_tx.send(common_config).is_err() {
                        error!(kind = "response channel sending");
                    }
                }

                info!(status = "terminating");
            }
            .instrument(info_span!("common_configuration_handler")),
        );

        (tx, task)
    }

    pub(crate) fn handle_partner_config(&self) -> (PartnerConfigChannel, JoinHandle<()>) {
        let (tx, mut rx) = roundtrip_channel::<PartnerConfigRequest, PartnerConfig>(10);
        let cloned_self = self.clone();

        let task = tokio::spawn(
            async move {
                info!(status = "started");

                while let Some((request, _, reply_tx)) = rx.recv().await {
                    let Ok(partner_config) = cloned_self.query(Some(&request.id)).await else {
                        continue;
                    };
                    if reply_tx.send(partner_config).is_err() {
                        error!(kind = "response channel sending");
                    }
                }

                info!(status = "terminating");
            }
            .instrument(info_span!("partner_configuration_handler")),
        );

        (tx, task)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use indoc::indoc;
    use mockito::{Mock, Server};

    use super::*;

    mod query {
        use super::*;

        fn server_mock(server: &mut Server, path: &str) -> Mock {
            server
                .mock("GET", path)
                .match_header("Accept", "application/json")
        }

        #[tokio::test]
        async fn url_join_error() {
            let mut server = Server::new_async().await;
            let mock = server_mock(&mut server, "/common")
                .with_status(200)
                .with_body("")
                .with_header("content-type", "application/json")
                .expect_at_most(0)
                .create_async()
                .await;
            let config = Config {
                config_api_url: "mailto:someone".parse().unwrap(),
                common_config_cache_expiration: Duration::ZERO.into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let result = client.query::<()>(None).await;
            assert!(result.is_err());
            mock.assert_async().await;
        }

        #[tokio::test]
        async fn request_send_error() {
            let mut server = Server::new_async().await;
            let mock = server_mock(&mut server, "/common")
                .with_status(200)
                .with_body("")
                .with_header("content-type", "application/json")
                .expect_at_most(0)
                .create_async()
                .await;
            let config = Config {
                config_api_url: "ftp://example.com".parse().unwrap(),
                common_config_cache_expiration: Duration::ZERO.into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let result = client.query::<()>(None).await;
            assert!(result.is_err());
            mock.assert_async().await;
        }

        #[tokio::test]
        async fn bad_status_code() {
            let mut server = Server::new_async().await;
            let mock = server_mock(&mut server, "/common")
                .with_status(500)
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
                common_config_cache_expiration: Duration::ZERO.into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let result = client.query::<()>(None).await;
            assert!(result.is_err());
            mock.assert_async().await;
        }

        #[tokio::test]
        async fn json_deserialization_error() {
            let mut server = Server::new_async().await;
            let mock = server_mock(&mut server, "/common")
                .with_status(200)
                .with_body("[")
                .with_header("content-type", "application/json")
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
                common_config_cache_expiration: Duration::ZERO.into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let result = client.query::<Vec<()>>(None).await;
            assert!(result.is_err());
            mock.assert_async().await;
        }

        #[tokio::test]
        async fn success_common() {
            let mut server = Server::new_async().await;
            let mock = server_mock(&mut server, "/common")
                .with_status(200)
                .with_body("[null]")
                .with_header("content-type", "application/json")
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
                common_config_cache_expiration: Duration::ZERO.into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let result = client.query::<Vec<()>>(None).await;
            assert_eq!(result.unwrap(), vec![()]);
            mock.assert_async().await;
        }

        #[tokio::test]
        async fn sucess_partner() {
            let mut server = Server::new_async().await;
            let mock = server_mock(&mut server, "/someid")
                .with_status(200)
                .with_body("[null,null]")
                .with_header("content-type", "application/json")
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
                common_config_cache_expiration: Duration::ZERO.into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let result = client.query::<Vec<()>>(Some("/someid")).await;
            assert_eq!(result.unwrap(), vec![(), ()]);
            mock.assert_async().await;
        }
    }

    mod common_config {
        use tokio::task::JoinSet;

        use super::*;

        fn success_fixture() -> (&'static str, CommonConfig) {
            let body = indoc! {r#"{
                "shiftStartTimes": ["01:02:03", "04:05:06"],
                "pauses": [
                    ["07:08:09", "10:11:12"],
                    ["13:14:15", "16:17:18"]
                ],
                "weekStart": {
                    "day": "Monday",
                    "shiftIndex": 0
                }
            }"#};
            let common_config = CommonConfig {
                shift_start_times: vec!["01:02:03".parse().unwrap(), "04:05:06".parse().unwrap()],
                pauses: vec![
                    ("07:08:09".parse().unwrap(), "10:11:12".parse().unwrap()),
                    ("13:14:15".parse().unwrap(), "16:17:18".parse().unwrap()),
                ],
                week_start: WeekStart {
                    day: Weekday::Mon,
                    shift_index: 0,
                },
            };
            (body, common_config)
        }

        #[tokio::test]
        async fn query_error() {
            let config = Config {
                config_api_url: "mailto:someone".parse().unwrap(),
                common_config_cache_expiration: Duration::ZERO.into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let result = client.cached_common_config().await;
            assert!(result.is_err());
        }

        #[tokio::test]
        async fn shift_start_times_not_sorted() {
            let mut server = Server::new_async().await;
            let mock = server
                .mock("GET", "/common")
                .with_status(200)
                .with_body(indoc! {r#"{
                    "shiftStartTimes": ["04:05:06", "01:02:03"],
                    "pauses": [
                        ["07:08:09", "10:11:12"],
                        ["13:14:15", "16:17:18"]
                    ],
                    "weekStart": {
                        "day": "Monday",
                        "shiftIndex": 0
                    }
                }"#})
                .with_header("content-type", "application/json")
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
                common_config_cache_expiration: Duration::ZERO.into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let result = client.cached_common_config().await;
            assert!(result.is_err());
            mock.assert_async().await;
        }

        #[tokio::test]
        async fn week_start_shift_index_out_of_bounds() {
            let mut server = Server::new_async().await;
            let mock = server
                .mock("GET", "/common")
                .with_status(200)
                .with_body(indoc! {r#"{
                    "shiftStartTimes": ["01:02:03", "04:05:06"],
                    "pauses": [
                        ["07:08:09", "10:11:12"],
                        ["13:14:15", "16:17:18"]
                    ],
                    "weekStart": {
                        "day": "Monday",
                        "shiftIndex": 2
                    }
                }"#})
                .with_header("content-type", "application/json")
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
                common_config_cache_expiration: Duration::ZERO.into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let result = client.cached_common_config().await;
            assert!(result.is_err());
            mock.assert_async().await;
        }

        #[tokio::test]
        async fn success_cache_hit_simultaneous() {
            let (body, expected) = success_fixture();
            let mut server = Server::new_async().await;
            let mock = server
                .mock("GET", "/common")
                .with_status(200)
                .with_body(body)
                .with_header("content-type", "application/json")
                .expect_at_most(1)
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
                common_config_cache_expiration: Duration::from_millis(15).into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            const QUERIES: usize = 10;
            let mut join_set = JoinSet::new();
            for _ in 0..QUERIES {
                let client = client.clone();
                join_set.spawn(async move { client.cached_common_config().await });
            }
            let mut seen = 0;
            while let Some(result) = join_set.join_next().await {
                let config = result.unwrap().unwrap();
                seen += 1;
                assert_eq!(config, expected);
            }
            assert_eq!(seen, QUERIES);
            mock.assert_async().await;
        }

        #[tokio::test]
        async fn success_cache_hit_successive() {
            let (body, expected) = success_fixture();
            let mut server = Server::new_async().await;
            let mock = server
                .mock("GET", "/common")
                .with_status(200)
                .with_body(body)
                .with_header("content-type", "application/json")
                .expect_at_most(1)
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
                common_config_cache_expiration: Duration::from_millis(100).into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(5)).await;
                let config = client.cached_common_config().await.unwrap();
                assert_eq!(config, expected);
            }
            mock.assert_async().await;
        }

        #[tokio::test]
        async fn success_cache_missed() {
            let (body, expected) = success_fixture();
            let mut server = Server::new_async().await;
            let mock = server
                .mock("GET", "/common")
                .with_status(200)
                .with_body(body)
                .with_header("content-type", "application/json")
                .expect_at_least(10)
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
                common_config_cache_expiration: Duration::from_millis(10).into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(15)).await;
                let config = client.cached_common_config().await.unwrap();
                assert_eq!(config, expected);
            }
            mock.assert_async().await;
        }
    }

    mod handle_partner_config {
        use super::*;

        #[tokio::test]
        async fn query_error() {
            let config = Config {
                config_api_url: "mailto:someone".parse().unwrap(),
                common_config_cache_expiration: Duration::ZERO.into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let request = PartnerConfigRequest {
                id: "testid".to_string(),
            };
            let (config_channel, task) = client.handle_partner_config();
            assert!(config_channel.roundtrip(request).await.is_err());
            assert!(!task.is_finished());
        }

        #[tokio::test]
        async fn success() {
            let mut server = Server::new_async().await;
            let mock = server
                .mock("GET", "/testid")
                .with_status(200)
                .with_body(indoc! {r#"{
                    "targetCycleTime": 42.42,
                    "targetEfficiency": 54.65,
                    "shiftEngaged": [true, false, true, true]
                }"#})
                .with_header("content-type", "application/json")
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
                common_config_cache_expiration: Duration::ZERO.into(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let request = PartnerConfigRequest {
                id: "testid".to_string(),
            };
            let (config_channel, task) = client.handle_partner_config();
            let config = config_channel.roundtrip(request).await.unwrap();
            assert_eq!(
                config,
                PartnerConfig {
                    target_cycle_time: 42.42,
                    target_efficiency: 54.65,
                    shift_engaged: vec![true, false, true, true]
                }
            );
            mock.assert_async().await;
            assert!(!task.is_finished());
        }
    }
}
