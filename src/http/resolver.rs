use hyper::{client::connect::dns::Name, service::Service};
use std::{
    future::Future,
    io,
    net::{SocketAddr, ToSocketAddrs},
    pin::Pin,
    task::{Context, Poll},
};
use trust_dns_resolver::TokioAsyncResolver;

#[derive(Debug, Clone)]
pub(crate) struct AsyncResolver(TokioAsyncResolver);

impl AsyncResolver {
    pub(crate) fn new() -> Result<Self, io::Error> {
        let resolver = TokioAsyncResolver::tokio_from_system_conf()?;
        Ok(Self(resolver))
    }
}

impl Service<Name> for AsyncResolver {
    type Response = std::vec::IntoIter<SocketAddr>;
    type Error = io::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, name: Name) -> Self::Future {
        let resolver = self.0.clone();

        Box::pin(async move {
            Ok(resolver
                .lookup_ip(name.as_str())
                .await?
                .iter()
                .map(|addr| (addr, 0_u16).to_socket_addrs())
                .try_fold(Vec::new(), |mut acc, s_addr| {
                    acc.extend(s_addr?);
                    Ok::<_, io::Error>(acc)
                })?
                .into_iter())
        })
    }
}
