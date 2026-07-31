use crate::prelude::*;

use esp_idf_svc::{
	hal::modem::Modem,
	eventloop::EspSystemEventLoop,
	nvs::{EspNvsPartition, NvsDefault},
	timer::EspTaskTimerService,
	wifi::{AsyncWifi, EspWifi},
};

use embedded_svc::wifi::{AccessPointConfiguration, AuthMethod, Configuration as WifiConfiguration};

////////////////////////////////////////////////////////////////////////////////

/// Setup Wi-Fi network. Returns a Wi-Fi handle, which needs to be kept alive.
pub async fn initialize_wifi<'a>(
	modem: Modem<'a>,
	sys_loop: &EspSystemEventLoop,
	timer_service: &EspTaskTimerService,
	nvs: &EspNvsPartition<NvsDefault>,
	ssid: heapless::String<32>,
	password: heapless::String<64>,
) -> Result<AsyncWifi<EspWifi<'a>>, AnyhowError> {
	let mut wifi = AsyncWifi::wrap(
		EspWifi::new(modem, sys_loop.clone(), Some(nvs.clone()))?,
		sys_loop.clone(),
		timer_service.clone(),
	)?;

	wifi.start().await.map_err(damn!("Error starting Wifi service"))?;
	info!("Wifi service started");

	// Create this earlier because the config struct takes ownership
	let format_string = format!("SSID `{ssid}', password `{password}'");

	wifi.set_configuration(&WifiConfiguration::AccessPoint(AccessPointConfiguration {
		ssid, password, auth_method: AuthMethod::WPA2Personal,
		..Default::default()
	})).map_err(damn!("Wifi configuration failure"))?;

	wifi.wait_netif_up().await
		.map_err(damn!("Wifi network await failure"))?;

	let ip = wifi.wifi().ap_netif().get_ip_info()
		.map_err(damn!("Failed to get IP information"))?.ip;

	info!("Network up, with {format_string}, gateway IP {ip}");

	Ok(wifi)
}
