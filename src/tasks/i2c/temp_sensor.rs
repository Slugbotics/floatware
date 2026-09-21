use crate::{
	prelude::*,
	tasks::i2c::{TempHumidityResponse, TIMEOUT_1S}
};

use esp_idf_svc::hal::i2c::I2cDriver;

/// SHTC3
pub mod th_sensor {
	pub const ADDR: u8 = 0x70;

	pub const CMD_RESET: [u8; 2] = [0x80, 0x5D];
	pub const CMD_WAKE:  [u8; 2] = [0x35, 0x17];
	pub const CMD_MEAS:  [u8; 2] = [0x78, 0x66];
	pub const CMD_SLEEP: [u8; 2] = [0xB0, 0x98];
}

pub fn reset_th_sensor(
	i2c: &mut I2cDriver<'_>,
) -> Result<(), AnyhowError> {
	i2c.write(th_sensor::ADDR, &th_sensor::CMD_RESET, TIMEOUT_1S)
		.map_err(AnyhowError::from)
}

#[inline]
pub(crate) async fn get_temp_humidity(
	i2c: &mut I2cDriver<'_>,
) -> Result<TempHumidityResponse, AnyhowError> {
	debug!("Waking up");
	// Wake up the SHTC3
	i2c.write(th_sensor::ADDR, &th_sensor::CMD_WAKE, TIMEOUT_1S).map_err(damn!("Wake fail"))?;
	// TODO: check delay

	sleep_ms!(20);

	debug!("Requesting measurement");

	// Request measurement
	i2c.write(th_sensor::ADDR, &th_sensor::CMD_MEAS, TIMEOUT_1S).map_err(damn!("Request fail"))?;
	sleep_ms!(20); // probably excessive

	let mut meas: [u8; 6] = [0; 6];

	debug!("Reading measurement");

	// Read measurement
	i2c.read(th_sensor::ADDR, &mut meas, TIMEOUT_1S).map_err(damn!("Read fail"))?;

	if crc_shtc3(meas[0], meas[1]) != meas[2] {
		warn!("I2C T&H: Temp failed CRC");
	}

	if crc_shtc3(meas[3], meas[4]) != meas[5] {
		warn!("I2C T&H: Humidity failed CRC");
	}

	let temp_raw = (meas[0] as u32) << 8 | meas[1] as u32;
	let  hum_raw = (meas[3] as u32) << 8 | meas[4] as u32;

	let response = TempHumidityResponse {
		celsius: (((temp_raw * 175) >> 16) - 45) as u8,
		relative_humidity: ((hum_raw * 100) >> 16) as u8
	};

	debug!("{response:?}");

	debug!("Putting to sleep");
	// Back to sleep
	i2c.write(th_sensor::ADDR, &th_sensor::CMD_SLEEP, TIMEOUT_1S).map_err(damn!("Sleep fail"))?;

	info!("Done");

	Ok(response)
}

/// SHTC3 responses include a crc byte which we verify.
fn crc_shtc3(msb: u8, lsb: u8) -> u8 {
	let mut crc = 0xFF;
	crc ^= msb;
	for _ in 0..8 {
		crc = (crc << 1) ^ (if crc & 0x80 != 0 { 0x31 } else { 0 })
	}
	crc ^= lsb;
	for _ in 0..8 {
		crc = (crc << 1) ^ (if crc & 0x80 != 0 { 0x31 } else { 0 })
	}
	crc
}