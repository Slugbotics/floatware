use crate::prelude::*;

use esp_idf_svc::hal::gpio::{Gpio9, PinDriver, Pull};

////////////////////////////////////////////////////////////////////////////////

/// Sends out a signal containing the state of the BOOT button on the board when
/// it's pressed.
pub async fn boot_button_pressed_task(
	boot_button: Gpio9<'_>,
	// TODO: communication channel
) -> Never {
	let mut driver = PinDriver::input(boot_button, Pull::Up)
		.map_err(damn!("Failed to initialize boot button input driver"))?;

	loop {
		driver.wait_for_any_edge().await.map_err(damn!("Edge await failed"))?;

		let state = driver.is_high();

		info!("Boot button {}pressed", if state { "" } else { "un" });
	}
}
