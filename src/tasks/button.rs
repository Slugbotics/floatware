//! This task reads the state of the "BOOT" button on the ESP rust board. When
//! pressed, it measures the amount of time that the button is held for, and when
//! the button is released, different actions are taken based on held duration.
//! Additionally, the LED changes while the button is held in order to indicate what
//! will happen when it is released.

use crate::prelude::*;

use esp_idf_svc::hal::gpio::{Gpio9, PinDriver, Pull};

use embassy_time::{Duration, WithTimeout};
use smart_leds_trait::RGB8;

////////////////////////////////////////////////////////////////////////////////

#[derive(Copy, Clone)]
struct Con(Duration, RGB8);

const impl Con {
	fn from_list<const N: usize>(lst: [(u64, RGB8); N]) -> [Self; N] {
		let mut prev = 0;

		// Since this function is `const`, these elements will never actually exist (I think)
		let mut ret = [Self(Duration::MIN, led::NONE); N];

		let mut i = 0;
		while i < N {
			let (ms, led) = lst[i];
			ret[i] = Self(Duration::from_millis(ms - prev), led);
			prev = ms;

			i += 1;
		}

		ret
	}
}

/// When holding the button, the LED will show the particular color for up to that
/// many milliseconds after the button begins to be held. They are time-delimiters
/// for the actions shown in-between.
const DURATIONS: [Con; 3] = Con::from_list([
	// None, guard against accidental presses
	( 200, led::NONE),
	// TBD
	(3000, led::NONE),
	// Drop UART
	(6000, led::PINK),
	// None; if you hold the button for too long, you can keep holding it until it resets to noop.
]);

pub async fn boot_button_pressed_task(
	boot_button: Gpio9<'_>,
	led_color_signal: &LedColorSignal,
	release_uart_signal: UartReleaseSender<'_>,
) -> Never {
	let mut driver = PinDriver::input(boot_button, Pull::Up)
		.map_err(damn!("Failed to initialize boot button input driver"))?;

	loop {
		// Button is active low
		driver.wait_for_falling_edge().await.map_err(damn!("Press await failed"))?;

		// Iterate over all the durations. For each one, set the LED to that color, 
		for (index, Con(duration, led)) in DURATIONS.iter().enumerate() {
			// Set LED to indicate what will happen if you release the button
			led_color_signal.signal(*led);

			if let Ok(result) =
				driver.wait_for_rising_edge()
				.with_timeout(*duration)
				.await
			{
				// Button released
				result.map_err(damn!("Unpress await failed"))?;
				match index {
					0 => {/* [0, 200) -> nothing; guard */},
					1 => {
						// [200, 3000) -> TBD
					},
					2 => {
						// [3000, 6000) -> release UART
						release_uart_signal.send(());
					},
					_ => unreachable!(),
				}
				break;
			} // else: timed out, go to next
		}
		// Went past last item (or broke after action); overflow/reset to doing nothing

		led_color_signal.reset();
		// Note that signal reset != none. When it is none, the LED
		// will (probably) flash, as opposed to not changing at all
	}
}
