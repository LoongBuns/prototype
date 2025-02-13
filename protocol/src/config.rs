use alloc::sync::Arc;

#[derive(Debug, Clone)]
pub struct Wifi {
    pub ssid: Arc<str>,
    pub password: Arc<str>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub host: Arc<str>,
    pub port: u16,
    pub wifi: Option<Wifi>,
}

impl Config {
    pub fn new() -> Self {
        let wifi = option_env!("WIFI_SSID")
            .zip(option_env!("WIFI_PASSWORD"))
            .map(|(ssid, password)| Wifi {
                ssid: Arc::from(ssid),
                password: Arc::from(password),
            });

        Self {
            wifi,
            ..Default::default()
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: Arc::from("0.0.0.0"),
            port: 3000,
            wifi: None,
        }
    }
}
