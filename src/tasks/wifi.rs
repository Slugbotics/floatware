use std::convert::TryInto;

use esp_idf_svc::{
	hal::modem::Modem,
	eventloop::EspSystemEventLoop,
	nvs::{EspNvsPartition, NvsDefault},
	timer::EspTaskTimerService,
	wifi::{AsyncWifi, EspWifi},
};

use anyhow::Error as AnyhowError;

use embedded_svc::wifi::{AccessPointConfiguration, AuthMethod, Configuration as WifiConfiguration};

use log::{error, info};

////////////////////////////////////////////////////////////////////////////////

/// Self-explanatory name. Takes ownership of the modem. I don't know what the
/// lifetime specifiers mean and at this point I'm too afraid to ask.
/// TODO: convert hardcoded SSID & pass to external config
pub async fn initialize_wifi<'a>(
	modem: Modem<'a>,
	sys_loop: &EspSystemEventLoop,
	timer_service: &EspTaskTimerService,
	nvs: &EspNvsPartition<NvsDefault>,
) -> Result<AsyncWifi<EspWifi<'a>>, AnyhowError> {
	let mut wifi = AsyncWifi::wrap(
		EspWifi::new(modem, sys_loop.clone(), Some(nvs.clone()))?,
		sys_loop.clone(),
		timer_service.clone(),
	)?;

	wifi.start().await?;
	info!("Wifi service started");

	wifi.set_configuration(&WifiConfiguration::AccessPoint(AccessPointConfiguration {
		ssid: "ESP".try_into().unwrap(),
		password: "floatware".try_into().unwrap(),
		auth_method: AuthMethod::WPA2Personal,
		..Default::default()
	}))?;

	wifi.wait_netif_up().await?;

	match wifi.wifi().ap_netif().get_ip_info() {
		Ok(info) => {
			info!("Network up, with gateway IP {}", info.ip);
			Ok(wifi)
		},
		Err(err) => {
			error!("Failed to get IP information");
			Err(AnyhowError::from(err))
		},
	}
}
