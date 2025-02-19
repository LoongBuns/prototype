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
        let host = option_env!("HOST").map_or_else(
            || Arc::from("localhost"),
            |host_str| Arc::from(host_str),
        );

        let port = option_env!("PORT").map_or_else(
            || 3000,
            |port_str| port_str.parse::<u16>().unwrap_or_else(|_| 3000),
        );

        let wifi = option_env!("WIFI_SSID")
            .zip(option_env!("WIFI_PASSWORD"))
            .map(|(ssid, password)| Wifi {
                ssid: Arc::from(ssid),
                password: Arc::from(password),
            });

        Self {
            host,
            port,
            wifi,
        }
    }
}
