mod http;
mod plugins;
mod responses;

use anyhow::{Context, Result};
use graphql_hive_router::{registry::HiveRegistry, usage};

fn main() -> Result<()> {
    usage::register();

    HiveRegistry::new(None).context("failed to load GraphQL Hive registry")?;

    apollo_router::main()
}
