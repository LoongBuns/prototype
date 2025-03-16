use alloc::sync::Arc;

#[derive(Debug, Clone)]
pub struct Wifi {
    pub ssid: Arc<str>,
    pub password: Arc<str>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub host: Arc<str>,
    pub dispatcher_port: u16,
    pub inspector_port: u16,
    pub wifi: Option<Wifi>,
}

impl Config {
    pub fn new() -> Self {
        let host = option_env!("HOST").map_or(Arc::from("localhost"), Arc::from);

        let dispatcher_port = option_env!("WEB_PORT")
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(3030);

        let inspector_port = option_env!("INSPECTOR_PORT")
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(3000);

        let wifi = option_env!("WIFI_SSID")
            .zip(option_env!("WIFI_PASSWORD"))
            .map(|(ssid, password)| Wifi {
                ssid: Arc::from(ssid),
                password: Arc::from(password),
            });

        Self {
            host,
            dispatcher_port,
            inspector_port,
            wifi,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: Arc::from("localhost"),
            dispatcher_port: 3030,
            inspector_port: 3000,
            wifi: None,
        }
    }
}
