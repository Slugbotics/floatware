use crate::{
	prelude::*,
	tasks::i2c::{
		DepthResponse,
		TIMEOUT_1S
	}
};

use esp_idf_svc::hal::i2c::I2cDriver;

use cfor::cfor;

/// MS5837
mod p_sensor {
	pub const ADDR: u8 = 0x76;

	pub const CMD_RESET: u8 = 0x1E;
	/// Must be ORed with the offset (0x00 -- 0x0C, even bytes only). Returns 2 bytes.
	pub const CMD_READ_PROM: u8 = 0xA0;
	pub const fn read_prom(offset: usize) -> u8 {
		CMD_READ_PROM | ((offset as u8) & 0x0C)
	}

	pub const CMD_READ_ADC: u8 = 0x00;

	pub const CMD_CONVERT_D1: u8 = 0x40;
	pub const CMD_CONVERT_D2: u8 = 0x50;
}

#[inline]
pub fn pressure_sensor_reset_and_read_prom(
	i2c: &mut I2cDriver<'_>
) -> Result<[u8; 14], AnyhowError> {
	i2c.write(
		p_sensor::ADDR, &[p_sensor::CMD_RESET], TIMEOUT_1S
	).map_err(damn!("Pressure sensor reset failed"))?;

	// TODO: delay?

	// Read PROM data (ds. page 8)
	let mut prom = [0_u8; 14];

	for off in (0x00 ..= 0x0C).step_by(2) {
		i2c.write(p_sensor::ADDR, &[p_sensor::read_prom(off)], TIMEOUT_1S)
			.map_err(damn!("Pressure sensor PROM request failed at offset 0x{:02X}", off))?;
		i2c.read(p_sensor::ADDR, &mut prom[off .. off+2], TIMEOUT_1S)
			.map_err(damn!("Pressure sensor PROM read failed at offset 0x{:02X}", off))?;
	}

	Ok(prom)
}

#[inline] pub async fn get_pressure(
	i2c: &mut I2cDriver<'_>,
) -> Result<DepthResponse, AnyhowError> {
	//! TODO
	Ok(Default::default())
}

/// Adapted from pressure sensor datasheet page 10:
///
/// ```c
/// unsigned char crc4(unsigned int n_prom[]) { // n_prom defined as 8x unsigned int (n_prom[8])
/// 	int cnt; // simple counter
/// 	unsigned int n_rem=0; // crc remainder
/// 	unsigned char n_bit;
/// 	n_prom[0]=((n_prom[0]) & 0x0FFF); // CRC byte is replaced by 0
/// 	n_prom[7]=0; // Subsidiary value, set to 0
/// 	for (cnt = 0; cnt < 16; cnt++) {// operation is performed on bytes
/// 		// choose LSB or MSB
/// 		if (cnt%2==1) n_rem ^= (unsigned short) ((n_prom[cnt>>1]) & 0x00FF);
/// 		else n_rem ^= (unsigned short) (n_prom[cnt>>1]>>8);
/// 		for (n_bit = 8; n_bit > 0; n_bit--) {
/// 			if (n_rem & (0x8000)) n_rem = (n_rem << 1) ^ 0x3000;
/// 			else n_rem = (n_rem << 1);
/// 		}
/// 	}
/// 	n_rem= ((n_rem >> 12) & 0x000F); // final 4-bit remainder is CRC code
/// 	return (n_rem ^ 0x00);
/// }
/// ```
fn crc_ms5837(mut n_prom: [u16; 8]) -> u8 {
	n_prom[0] = n_prom[0] & 0x0FFF;
	n_prom[7] = 0;

	let mut n_rem = 0;

	for cnt in 0..16 {
		if cnt % 2 == 1 {
			n_rem ^= n_prom[cnt >> 1] & 0x00FF;
		} else {
			n_rem ^= n_prom[cnt >> 1] >> 8;
		}
		cfor!{let mut n_bit = 8; n_bit > 0; n_bit -= 1; {
			if n_rem & 0x8000 != 0 {
				n_rem = (n_rem << 1) ^ 0x3000;
			} else {
				n_rem = n_rem << 1;
			}
		}}
	}

	((n_rem >> 12) & 0x000F) as u8
}

/// TODO based on I2C depth sensor driver code
pub type Depth = f32;