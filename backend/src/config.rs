use figment::{
    providers::{Env, Format, Serialized, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AppConfig {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    pub session_secret: String,
    pub session_ttl_hours: u64,
    pub resend_api_key: String,
    pub resend_from: String,
    pub app_base_url: String,
    pub admin_bootstrap_email: Option<String>,
    pub admin_bootstrap_password: Option<String>,
    pub data_dir: PathBuf,
    pub model_dir: PathBuf,
    pub worker_threads: usize,
    pub dev_cors: bool,
    pub max_concurrent_generation: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:8080".parse().expect("valid default addr"),
            database_url: String::new(),
            session_secret: String::new(),
            session_ttl_hours: 24 * 14, // 14 days
            resend_api_key: String::new(),
            resend_from: String::new(),
            app_base_url: String::new(),
            admin_bootstrap_email: None,
            admin_bootstrap_password: None,
            data_dir: PathBuf::from("/data"),
            model_dir: PathBuf::from("/data/models"),
            worker_threads: (num_cpus::get().saturating_sub(1)).max(1),
            dev_cors: false,
            max_concurrent_generation: 2,
        }
    }
}

impl AppConfig {
    #[allow(clippy::result_large_err)] // figment::Error is a library type we can't box
    pub fn load() -> Result<Self, figment::Error> {
        Figment::from(Serialized::defaults(AppConfig::default()))
            .merge(Toml::file("app.toml"))
            .merge(Env::raw().map(|k| k.as_str().to_lowercase().into()))
            .extract()
    }
}
