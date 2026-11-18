mod client;
mod config;
mod proxy;

use std::{convert::Infallible, error::Error, sync::Arc};

use hyper::{server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use config::Config;
use proxy::AppState;

type BoxError = Box<dyn Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let config = Config::from_env()?;
    let state = Arc::new(AppState::new(config)?);
    let listener = TcpListener::bind(state.config.listen_addr.as_str()).await?;

    eprintln!(
        "Proxify {} listening on {}, proxy path: {}",
        env!("CARGO_PKG_VERSION"),
        listener.local_addr()?,
        state.config.proxy_path
    );

    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();

        tokio::spawn(async move {
            let service = service_fn(move |request| {
                let state = state.clone();
                async move { Ok::<_, Infallible>(proxy::handle(request, state).await) }
            });

            if let Err(error) = http1::Builder::new()
                .serve_connection(TokioIo::new(stream), service)
                .await
            {
                eprintln!("HTTP connection error: {error}");
            }
        });
    }
}
