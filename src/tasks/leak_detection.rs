use crate::{
	prelude::*,
	tasks::shutdown::ShutdownRequest
};

use esp_idf_svc::{
	hal::gpio::{InputPin, PinDriver, Pull}
};

////////////////////////////////////////////////////////////////////////////////

/// If the provided GPIO pin (that to which the leak detector is connected to) ever
/// goes high, shut down the float.
pub async fn leak_detection_task(
	leak_detector_pin: impl InputPin,
	shutdown_signal: &ShutdownSignal,
) -> Void {
	let mut driver = PinDriver::input(leak_detector_pin, Pull::Down)
		.map_err(damn!("Failed to initialize leak detection pin"))?;

	// Suspend until high signal on the leak pin.
	driver.wait_for_high().await
		.map_err(damn!("Error occurred on leak detection interrupt"))?;

	// Send shutdown signal to shutdown handler.
	shutdown_signal.signal(ShutdownRequest { originator: "leak detection", go_to_surface: true });

	warn!("<<<<==== LEAK DETECTED ====>>>>");

	Ok(())
}
