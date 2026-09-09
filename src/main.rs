mod client;
mod config;
mod proxy;
mod version;

use std::{
    convert::Infallible,
    error::Error,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::Arc,
};

use hyper::{server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use tokio::net::UnixListener;

use config::Config;
use proxy::AppState;
use version::VERSION;

type BoxError = Box<dyn Error + Send + Sync>;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let config = Config::from_env();
    let socket_path = config.socket_path.clone();
    let proxy_path = config.proxy_path.clone();

    if let Some(parent) = Path::new(&socket_path).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    if tokio::fs::metadata(&socket_path).await.is_ok() {
        tokio::fs::remove_file(&socket_path).await?;
    }

    let listener = UnixListener::bind(&socket_path)?;
    tokio::fs::set_permissions(
        &socket_path,
        std::fs::Permissions::from_mode(0o666),
    )
    .await?;

    let state = Arc::new(AppState::new(config)?);

    eprintln!(
        "Proxify {} listening on socket {}, proxy path: {}",
        VERSION, socket_path, proxy_path
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
