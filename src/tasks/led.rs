use crate::{
	prelude::*,
};

use std::ops::Index;

use serde::Deserialize;

use ws2812_esp32_rmt_driver::{
	LedPixelEsp32Rmt,
	driver::color::LedPixelColorGrbw32
};

use smart_leds_trait::{SmartLedsWrite, RGB8};

////////////////////////////////////////////////////////////////////////////////

#[derive(Deserialize)]
#[serde(remote = "RGB8")]
pub struct _SerdeRGB8 { pub r: u8, pub g: u8, pub b: u8, }

macro_rules! color_const {
    ($name:ident, $r:expr, $g:expr, $b:expr) => {
		#[allow(unused)] pub const $name: RGB8 = RGB8::new($r, $g, $b);
	};
	($name:ident, $rgb:expr) => {
		color_const!($name, ((($rgb) & 0xFF0000) >> 16) as _, ((($rgb) & 0xFF00) >> 8) as _, ($rgb) as _);
	}
}

color_const!(  NONE, 0x000000);
color_const!(   DIM, 0x444444);
color_const!(   RED, 0xFF0000);
color_const!( GREEN, 0x00FF00);
color_const!(  BLUE, 0x0000FF);
color_const!(YELLOW, 0xFFFF00);
color_const!(ORANGE, 0xFF7700);

const LED_MAXIMUM_SIMULTANEOUS_STATES: usize = 6;

#[derive(Debug, Clone, PartialEq)]
pub struct LEDState {
	pub size: usize,
	pub list: [RGB8; LED_MAXIMUM_SIMULTANEOUS_STATES],
}

impl LEDState {
	pub fn new(list: &[RGB8]) -> Result<Self, AnyhowError> {
		let size = list.len();
		if size > LED_MAXIMUM_SIMULTANEOUS_STATES {
			Err(AnyhowError::msg("LED state size is too large"))
		} else {
			Ok(Self {
				size,
				list: list.try_into()?,
			})
		}
	}
}

impl Index<usize> for LEDState {
	type Output = RGB8;

	fn index(&self, index: usize) -> &Self::Output {
		self.list.get(index).unwrap_or(&NONE)
	}
}

// enum LEDStateOld {
// 	CharterUpdate(CharterState),
// 	StatusUpdate(SystemStatus)
// }
//
// impl LEDStateOld {
// 	fn get_led_color(&self) -> RGB8 {
// 		match self {
// 			Self::CharterUpdate(charter_state) => match charter_state {
// 				CharterState::StartRequested => BLUE,
// 				CharterState::InProgress {
// 					charter_index, target_depth: _, charter_size
// 				} => {
// 					// Fade from blue to green as the charter is completed
// 					let progress = (charter_index * 255 / charter_size) as u8; // 0 to 255
// 					RGB8::new(0, progress, 255 - progress)
// 				}
// 				CharterState::Completed => GREEN,
// 				CharterState::Aborted { .. } => RED,
// 			},
// 			Self::StatusUpdate(status) => {
//
// 			}
// 		}
// 	}
// }

////////////////////////////////////////////////////////////////////////////////

/// Lights the LED in accordance with the LED status set by the status generation
/// task. We iterate through the colors, and cycle the LED through them.
pub(crate) async fn led_task(
	mut led_driver: LedPixelEsp32Rmt<'_, RGB8, LedPixelColorGrbw32>,
	led_state_signal: &LedStateSignal,
) -> Never {
	let mut current_led_value = NONE;
	let mut current_led_state = LEDState::new(&[])?;
	let mut led_index = 0_usize;

	let mut set_led = |led| {
		if current_led_value != led {
			current_led_value = led;
			led_driver.write(std::iter::once(led))
				.unwrap_or_else(|error| error!("Error writing LED: {error:#}"));
		}
	};

	loop {
		// Status updates come every ~100ms.
		let new_led_state = led_state_signal.wait().await;

		// If we get a new state, then reset cycle.
		if new_led_state != current_led_state {
			current_led_state = new_led_state;
			led_index = 0;
		}

		// No states = keep LED off
		if current_led_state.size == 0 {
			set_led(NONE);

			continue;
		}

		if led_index >= current_led_state.size {
			led_index = 0;
		} else {
			led_index += 1;
		}

		set_led(current_led_state[led_index]);
	}
}
