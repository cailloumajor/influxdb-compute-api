use std::sync::Arc;

use clap::Args;
use reqwest::{header, Client as HttpClient};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tracing::{error, info, info_span, Instrument};
use url::Url;

use crate::model::ConfigFromApi;

#[derive(Args)]
#[group(skip)]
pub(crate) struct Config {
    /// Configuration API URL
    #[arg(env, long)]
    config_api_url: Url,
}

#[derive(Clone)]
pub(crate) struct Client {
    config_url: Arc<Url>,
    http_client: HttpClient,
}

pub(crate) struct ConfigRequest {
    pub(crate) id: String,
    pub(crate) response_channel: oneshot::Sender<ConfigFromApi>,
}

pub(crate) type ConfigChannel = mpsc::Sender<ConfigRequest>;

impl Client {
    pub(crate) fn new(config: &Config, http_client: HttpClient) -> Self {
        let config_url = Arc::new(config.config_api_url.clone());

        Self {
            config_url,
            http_client,
        }
    }

    pub(crate) fn handle_config(&self) -> (ConfigChannel, JoinHandle<()>) {
        let (tx, mut rx) = mpsc::channel::<ConfigRequest>(1);
        let cloned_self = self.clone();

        let task = tokio::spawn(
            async move {
                info!(status = "started");

                while let Some(request) = rx.recv().await {
                    let url = match cloned_self.config_url.join(&request.id) {
                        Ok(url) => url,
                        Err(err) => {
                            error!(kind = "joining config API URL and ID", %err);
                            continue;
                        }
                    };
                    let http_response = match cloned_self
                        .http_client
                        .get(url)
                        .header(header::ACCEPT, "application/json")
                        .send()
                        .await
                    {
                        Ok(resp) => resp,
                        Err(err) => {
                            error!(kind = "http request sending", %err);
                            continue;
                        }
                    };
                    let status_code = http_response.status();
                    if !status_code.is_success() {
                        error!(kind = "bad response status", %status_code);
                        continue;
                    }
                    let config_from_api = match http_response.json().await {
                        Ok(config) => config,
                        Err(err) => {
                            error!(kind = "response deserialization",%err);
                            continue;
                        }
                    };
                    if request.response_channel.send(config_from_api).is_err() {
                        error!(kind = "response channel sending");
                    }
                }

                info!(status = "terminating");
            }
            .instrument(info_span!("configuration_handler")),
        );

        (tx, task)
    }
}

#[cfg(test)]
mod tests {
    use mockito::{Mock, Server};

    use super::*;

    mod handle_config {
        use super::*;

        fn server_mock(server: &mut Server) -> Mock {
            server
                .mock("GET", "/testid")
                .match_header("Accept", "application/json")
        }

        #[tokio::test]
        async fn url_join_error() {
            let mut server = Server::new_async().await;
            let mock = server_mock(&mut server)
                .with_status(200)
                .with_body(r#"{"targetCycleTime":42.42,"targetEfficiency":54.65}"#)
                .with_header("content-type", "application/json")
                .expect_at_most(0)
                .create_async()
                .await;
            let config = Config {
                config_api_url: "mailto:someone".parse().unwrap(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let (tx, rx) = oneshot::channel();
            let request = ConfigRequest {
                id: "testid".to_string(),
                response_channel: tx,
            };
            let (config_channel, task) = client.handle_config();
            config_channel.send(request).await.unwrap();
            assert!(rx.await.is_err());
            mock.assert_async().await;
            assert!(!task.is_finished());
        }

        #[tokio::test]
        async fn request_send_error() {
            let mut server = Server::new_async().await;
            let mock = server_mock(&mut server)
                .with_status(200)
                .with_body(r#"{"targetCycleTime":42.42,"targetEfficiency":54.65}"#)
                .with_header("content-type", "application/json")
                .expect_at_most(0)
                .create_async()
                .await;
            let config = Config {
                config_api_url: "ftp://example.com".parse().unwrap(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let (tx, rx) = oneshot::channel();
            let request = ConfigRequest {
                id: "testid".to_string(),
                response_channel: tx,
            };
            let (config_channel, task) = client.handle_config();
            config_channel.send(request).await.unwrap();
            assert!(rx.await.is_err());
            mock.assert_async().await;
            assert!(!task.is_finished());
        }

        #[tokio::test]
        async fn bad_status_code() {
            let mut server = Server::new_async().await;
            let mock = server_mock(&mut server)
                .with_status(500)
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let (tx, rx) = oneshot::channel();
            let request = ConfigRequest {
                id: "testid".to_string(),
                response_channel: tx,
            };
            let (config_channel, task) = client.handle_config();
            config_channel.send(request).await.unwrap();
            assert!(rx.await.is_err());
            mock.assert_async().await;
            assert!(!task.is_finished());
        }

        #[tokio::test]
        async fn json_deserialization_error() {
            let mut server = Server::new_async().await;
            let mock = server_mock(&mut server)
                .with_status(200)
                .with_body("[")
                .with_header("content-type", "application/json")
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let (tx, rx) = oneshot::channel();
            let request = ConfigRequest {
                id: "testid".to_string(),
                response_channel: tx,
            };
            let (config_channel, task) = client.handle_config();
            config_channel.send(request).await.unwrap();
            assert!(rx.await.is_err());
            mock.assert_async().await;
            assert!(!task.is_finished());
        }

        #[tokio::test]
        async fn success() {
            let mut server = Server::new_async().await;
            let mock = server_mock(&mut server)
                .with_status(200)
                .with_body(r#"{"targetCycleTime":42.42,"targetEfficiency":54.65}"#)
                .with_header("content-type", "application/json")
                .create_async()
                .await;
            let config = Config {
                config_api_url: server.url().parse().unwrap(),
            };
            let http_client = HttpClient::new();
            let client = Client::new(&config, http_client);
            let (tx, rx) = oneshot::channel();
            let request = ConfigRequest {
                id: "testid".to_string(),
                response_channel: tx,
            };
            let (config_channel, task) = client.handle_config();
            config_channel.send(request).await.unwrap();
            let config = rx.await.unwrap();
            assert_eq!(
                config,
                ConfigFromApi {
                    target_cycle_time: 42.42,
                    target_efficiency: 54.65
                }
            );
            mock.assert_async().await;
            assert!(!task.is_finished());
        }
    }
}
