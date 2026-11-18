use std::{env, io};

#[derive(Debug)]
pub struct Config {
    pub proxy_path: String,
    pub listen_addr: String,
    pub jwt_public_key_file: String,
}

impl Config {
    pub fn from_env() -> io::Result<Self> {
        let jwt_public_key_file = env::var("JWT_PUBLIC_KEY_FILE")
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "JWT_PUBLIC_KEY_FILE is required",
                )
            })?;
        Ok(Self {
            proxy_path: env::var("PROXY_PATH").unwrap_or_else(|_| "/api/proxy/file".to_owned()),
            listen_addr: env::var("LISTEN").unwrap_or_else(|_| "127.0.0.1:18081".to_owned()),
            jwt_public_key_file,
        })
    }
}
