mod http;
mod plugins;
mod responses;

use anyhow::Result;

fn main() -> Result<()> {
    apollo_router::main()
}
