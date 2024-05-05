mod hive;
mod http;
mod plugins;
mod responses;

use anyhow::{Context, Result};
use apollo_router::{Executable, SchemaSource};
use graphql_hive_router::usage;
use tokio::runtime;

fn main() -> Result<()> {
    let mut builder = runtime::Builder::new_multi_thread();
    builder.enable_all();

    if let Some(nb) = std::env::var("APOLLO_ROUTER_NUM_CORES")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
    {
        builder.worker_threads(nb);
    }

    let runtime = builder.build().context("failed to configure runtime")?;
    runtime.block_on(inner_main())
}

async fn inner_main() -> Result<()> {
    usage::register();

    let schema = Box::pin(hive::schema().context("failed to load schema")?);

    Executable::builder()
        .schema(SchemaSource::Stream(schema))
        .start()
        .await
}
