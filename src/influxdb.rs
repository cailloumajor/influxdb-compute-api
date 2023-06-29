use std::io;

use clap::Args;
use csv_async::AsyncReaderBuilder;
use futures_util::{Stream, TryStreamExt};
use reqwest::header::{self, HeaderMap};
use reqwest::Client as HttpClient;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tracing::{error, instrument};
use url::Url;

#[derive(Deserialize)]
struct QueryResponse {
    message: String,
}

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
}

pub(crate) struct Client {
    url: Url,
    headers: HeaderMap,
    bucket: String,
    http_client: HttpClient,
}

impl Client {
    pub(crate) fn new(config: &Config) -> Self {
        let mut url = config.influxdb_url.join("/api/v2/query").unwrap();
        url.query_pairs_mut()
            .append_pair("org", &config.influxdb_org);
        let mut headers = HeaderMap::new();
        let auth_header = format!("Token {}", config.influxdb_api_token)
            .parse()
            .unwrap();
        headers.insert(header::ACCEPT, "application/csv".parse().unwrap());
        headers.insert(header::AUTHORIZATION, auth_header);
        headers.insert(
            header::CONTENT_TYPE,
            "application/vnd.flux".parse().unwrap(),
        );
        let bucket = config.influxdb_bucket.clone();
        let http_client = HttpClient::new();

        Self {
            url,
            headers,
            bucket,
            http_client,
        }
    }

    #[instrument(skip_all, name = "influxdb_query")]
    async fn query<T>(
        &self,
        flux_query: &'static str,
    ) -> Result<impl Stream<Item = Result<T, csv_async::Error>>, ()>
    where
        T: DeserializeOwned + 'static,
    {
        let body = flux_query.replacen("__bucketplaceholder__", &self.bucket, 1);

        let response = self
            .http_client
            .post(self.url.clone())
            .headers(self.headers.clone())
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
}

#[cfg(test)]
mod tests {
    use super::*;

    mod client {
        use super::*;

        mod query {
            use mockito::{Matcher, Mock, Server};

            use super::*;

            const FLUX_QUERY: &str = "some Flux query with __bucketplaceholder__";

            fn server_mock(server: &mut Server) -> Mock {
                server
                    .mock("POST", "/api/v2/query")
                    .match_query(Matcher::UrlEncoded("org".into(), "someorg".into()))
                    .match_header("Accept", "application/csv")
                    .match_header("Accept-Encoding", "gzip")
                    .match_header("Authorization", "Token sometoken")
                    .match_header("Content-Type", "application/vnd.flux")
                    .match_body("some Flux query with somebucket")
            }

            #[tokio::test]
            async fn request_send_failure() {
                let config = Config {
                    influxdb_url: "ftp://example.com".parse().unwrap(),
                    influxdb_api_token: "sometoken".to_string(),
                    influxdb_org: "someorg".to_string(),
                    influxdb_bucket: "somebucket".to_string(),
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
    }
}
