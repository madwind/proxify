use std::env;

#[derive(Debug)]
pub struct Config {
    pub proxy_path: String,
    pub socket_path: String,
    pub jwt_key: Vec<u8>,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            proxy_path: env::var("PROXY_PATH").unwrap_or_else(|_| "/api/proxy/file".to_owned()),
            socket_path: env::var("SOCKET_PATH")
                .unwrap_or_else(|_| "/dev/shm/proxify.sock".to_owned()),
            jwt_key: env::var("JWT_SIGNING_KEY")
                .unwrap_or_default()
                .into_bytes(),
        }
    }
}
