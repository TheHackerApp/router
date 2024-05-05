use anyhow::{Context, Result};
use futures::{Stream, StreamExt};
use http::{header, HeaderMap, HeaderValue, StatusCode};
use reqwest::Client;
use sha2::{Digest, Sha256};
use std::{env, time::Duration};
use tokio::sync::mpsc::channel;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{info_span, Instrument};
use url::Url;

static COMMIT: Option<&'static str> = option_env!("GITHUB_SHA");

pub(crate) fn schema() -> Result<impl Stream<Item = String> + Send> {
    let config = RegistryConfig::from_env()?;
    let (sender, receiver) = channel(2);

    let headers = {
        let mut map = HeaderMap::new();
        map.insert(
            header::USER_AGENT,
            HeaderValue::from_str(&format!("apollo-router/{}", COMMIT.unwrap_or("local"))).unwrap(),
        );
        map.insert(
            "X-Hive-CDN-Key",
            HeaderValue::from_str(&config.key).unwrap(),
        );
        map
    };
    let client = Client::builder().default_headers(headers).build()?;

    let task = async move {
        let mut etag = None;
        let mut last_schema = None;

        loop {
            let request = match etag.as_deref() {
                Some(etag) => client
                    .get(config.endpoint.as_str())
                    .header(header::IF_NONE_MATCH, HeaderValue::from_str(etag).unwrap()),
                None => client.get(config.endpoint.as_str()),
            };

            match request.send().await {
                Ok(response) => {
                    tracing::info!(
                        monotonic_counter.hive_registry_fetch_count_total = 1u64,
                        status = "success",
                    );

                    etag = response
                        .headers()
                        .get("etag")
                        .and_then(|etag| etag.to_str().ok())
                        .map(ToOwned::to_owned);

                    if response.status() != StatusCode::NOT_MODIFIED {
                        match response.text().await {
                            Ok(schema) => {
                                let schema_hash = Some(hash(schema.as_bytes()));
                                if schema_hash != last_schema {
                                    last_schema = schema_hash;
                                    if let Err(e) = sender.send(schema).await {
                                        tracing::debug!("failed to push to stream, router is likely shutting down: {e}");
                                        break;
                                    }
                                }
                            }
                            Err(err) => log_fetch_failure(err),
                        }
                    }
                }
                Err(err) => log_fetch_failure(err),
            };

            tokio::time::sleep(config.poll_interval).await;
        }
    };
    drop(tokio::task::spawn(task.instrument(info_span!("registry"))));

    Ok(ReceiverStream::new(receiver).boxed())
}

struct RegistryConfig {
    endpoint: Url,
    key: String,
    poll_interval: Duration,
}

impl RegistryConfig {
    fn from_env() -> Result<RegistryConfig> {
        let endpoint = env::var("HIVE_CDN_ENDPOINT")
            .context("missing HIVE_CDN_ENDPOINT environment variable")?;
        let endpoint = Url::parse(&endpoint).context("invalid CDN endpoint")?;

        let key = env::var("HIVE_CDN_KEY").context("missing HIVE_CDN_KEY environment variable")?;

        let poll_interval =
            env::var("HIVE_CDN_POLL_INTERVAL").unwrap_or_else(|_| String::from("10"));
        let poll_interval = Duration::from_secs(
            poll_interval
                .parse()
                .context("invalid poll interval format")?,
        );

        Ok(RegistryConfig {
            endpoint,
            key,
            poll_interval,
        })
    }
}

fn log_fetch_failure(err: impl std::fmt::Display) {
    tracing::info!(
        monotonic_counter.hive_registry_fetch_count_total = 1u64,
        status = "failure"
    );
    tracing::error!(code = "HIVE_REGISTRY_FETCH_FAILURE", "{}", err);
}

fn hash(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().into()
}
