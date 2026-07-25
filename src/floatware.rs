use crate::{
	signals::*,
	tasks::{
		http,
		led,
		shutdown,
		wifi,
	}
};

use std::sync::Arc;

use esp_idf_svc::{
	eventloop::EspSystemEventLoop,
	hal::{
		peripherals::Peripherals,
		task::embassy_sync::EspRawMutex, // Safe across threads but not ISRs
	},
	http::server::EspHttpServer,
	nvs::EspDefaultNvsPartition,
	sys::EspError,
	timer::EspTaskTimerService,
};

use log::{error, info, warn};

use anyhow::Error as AnyhowError;

use ws2812_esp32_rmt_driver::{driver::color::LedPixelColorGrbw32, LedPixelEsp32Rmt, RGB8};

////////////////////////////////////////////////////////////////////////////////

/// Where we actually start setting up everything. This function is responsible for
/// spawning the remaining threads, initializing peripherals, and spawning tasks.
pub async fn initialize() -> Result<(), AnyhowError> {
	let peripherals = Peripherals::take()?;
	let sys_loop = EspSystemEventLoop::take()?;
	let timer_service = EspTaskTimerService::new()?;
	let nvs = EspDefaultNvsPartition::take()?;

	let _wifi = match wifi::initialize_wifi(
		peripherals.modem, &sys_loop, &timer_service, &nvs
	).await {
		Ok(wifi) => wifi,
		Err(error) => {
			error!("Unable to initialize WiFi");
			return Err(error);
		},
	};

	// Used to update the state of the onboard RGB LED
	let led_signal = Arc::new(LedSignal::new());
	// Used to tell this function to exit
	let shutdown_signal = Arc::new(ShutdownSignal::new());

	let _http_server: EspHttpServer = http::initialize_http_server(
		Arc::clone(&led_signal),
		Arc::clone(&shutdown_signal),
	)?;

	let led_driver = LedPixelEsp32Rmt::<RGB8, LedPixelColorGrbw32>::new(
		peripherals.rmt.channel0, peripherals.pins.gpio2
	)?;

	info!("Initialization complete. Starting event loop.");

	// Any task returning an error leads to an immediate shutdown
	if let Err(error) = futures::try_join!(
		led::led_task(led_driver, Arc::clone(&led_signal)),
		shutdown::shutdown_task(Arc::clone(&shutdown_signal))
	) {
		// The shutdown task has a unique error that it returns to indicate that everything is fine.
		if !error.root_cause().is::<shutdown::Shutdown>() {
			return Err(error);
		}
	}

	Ok(())
}

