use crate::signals::LedSignal;

use std::sync::Arc;

use anyhow::Error as AnyhowError;

use log::error;

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

pub(crate) async fn led_task(
	mut led_driver: LedPixelEsp32Rmt<'_, RGB8, LedPixelColorGrbw32>,
	led_signal: Arc<LedSignal>
) -> Result<(/* Never */), AnyhowError> {
	loop {
		let res = led_signal.wait().await;
		led_driver.write(std::iter::once(res))
			.unwrap_or_else(|error| error!("Error writing to LED: {error:#}"));
	}
}
