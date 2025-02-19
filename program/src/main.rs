mod container;

use std::io;

use container::{execute_wasm, setup_container};
use esp_idf_svc::{eventloop, hal, log as esp_log, nvs, sys, wifi};
use log::{error, info};
use protocol::{Config, Error as ProtocolError, Type, Wifi};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("protocol: {0}")]
    ProtocolError(#[from] ProtocolError),
    #[error("iwasm: {0}")]
    ContainerError(#[from] wamr_rust_sdk::RuntimeError),
    #[error("io: {0}")]
    IoError(#[from] io::Error),
}

fn setup_wifi(ssid: &str, password: &str) -> Result<wifi::EspWifi<'static>, sys::EspError> {
    let sys_loop = eventloop::EspSystemEventLoop::take()?;
    let nvs = nvs::EspDefaultNvsPartition::take()?;

    let peripherals = hal::prelude::Peripherals::take()?;

    let mut esp_wifi = wifi::EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs.clone()))?;
    let mut wifi = wifi::BlockingWifi::wrap(&mut esp_wifi, sys_loop.clone())?;

    wifi.set_configuration(&wifi::Configuration::Client(wifi::ClientConfiguration {
        ssid: ssid.try_into().unwrap(),
        password: password.try_into().unwrap(),
        ..Default::default()
    }))?;

    wifi.start()?;
    wifi.connect()?;
    wifi.wait_netif_up()?;

    Ok(esp_wifi)
}

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_log::EspLogger::initialize_default();

    let Config { host, port, wifi } = Config::new();

    if let Some(Wifi { ssid, password }) = wifi {
        match setup_wifi(&ssid, &password) {
            Ok(_) => {
                info!("Wifi connected");
                if let Err(err) = setup_container(&host, port) {
                    error!("Container error: {err}");
                }
            }
            Err(err) => error!("Wifi setup failed: {err}"),
        }
    } else {
        // If no wifi, debug wasm runtime
        // (module
        //   (func (export "run") (param i32 i32) (result i32)
        //     (local.get 0)
        //     (local.get 1)
        //     (i32.add)
        //   )
        // )
        let binary = vec![
            0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f,
            0x7f, 0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x72, 0x75, 0x6e,
            0x00, 0x00, 0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
        ];
        let binary = binary.into_iter().map(|c| c as u8).collect::<Vec<u8>>();
        let params: Vec<Type> = vec![Type::I32(10), Type::I32(20)];
        
        match execute_wasm(binary, params) {
            Ok(result) => {
                match result.first() {
                    Some(value) => info!("10 + 20 = {:?}", value),
                    None => error!("Wasm runtime execute fail with void result"),
                }
            }
            Err(err) => error!("Wasm runtime crashed: {err}"),
        }
    }
}
