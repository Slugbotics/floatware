use crate::{
	prelude::*,
	tasks::i2c::{PowerResponse, TIMEOUT_1S}
};

use esp_idf_svc::hal::i2c::I2cDriver;

mod power_sensor {
	/// The INA260 is a big-endian device.
	pub const ADDR: u8 = 0x40; // A0 = A1 = GND

	// All registers are 16-bit

	/// Register format: RxxxAAAV VVIIIMBS
	/// R: Reset active high
	/// AAA:  Average mode
	/// 	000:    1
	/// 	001:    4
	/// 	010:   16
	/// 	011:   64
	/// 	100:  128
	/// 	101:  256
	/// 	110:  512
	/// 	111: 1024
	/// VVV = Voltage conversion time
	/// III = Current conversion time (ditto)
	/// 	000:  140 us
	/// 	001:  204 us
	/// 	010:  332 us
	/// 	011:  588 us
	/// 	100: 1100 us
	/// 	101: 2116 us
	/// 	110: 4156 us
	/// 	111: 8244 us
	/// M = Continuous mode active high
	/// B = Measure bus voltage
	/// S = Measure shunt voltage
	pub const REG_CONFIG: u8 = 0x00;
	pub const REG_CURRENT: u8 = 0x01;
	pub const REG_VOLTAGE: u8 = 0x02;
	pub const REG_POWER: u8 = 0x03;
	pub const REG_ALERT_SOURCE: u8 = 0x06;
	pub const REG_ALERT_SETPOINT: u8 = 0x07;
	pub const REG_MFG_ID: u8 = 0xFE;
	pub const REG_DEVICE_ID: u8 = 0xFF;
}

#[inline]
pub(crate) async fn reset_ina260(
	i2c: &mut I2cDriver<'_>
) -> Result<(), AnyhowError> {
	i2c.write(power_sensor::ADDR, &[
		power_sensor::REG_CONFIG, 0b_1000_0000, 0b_0000_0000
	], TIMEOUT_1S).map_err(damn!("INA260 reset failed"))?;

	// The data sheet doesn't specify reset time
	sleep_ms!(100);

	// We have two factors that influence the conversion accuracy: conversion time, and
	// average count.

	// We want to have readings every 100ms. Currently snapshots are taken every 500ms
	// but I want to be able to decrease it.
	// The voltage will be relatively stable, so high accuracy is not super important

	// t_sample = avg * (t_v + t_i) = 64 * (140us + 1.1ms) = 79.36 ms

	i2c.write(power_sensor::ADDR, &[
		power_sensor::REG_CONFIG,
		// Avg=64 --\
		//        |||
		0b_0_000__011_0,
		//            |
		// /- V=140us /
		// ||     /-- Mode = Continuous V & I
		// ||     |||
		0b_00_100_111
		//    |||
		//    \-- I=1.1ms
	], TIMEOUT_1S).map_err(damn!("INA260 config failed"))?;

	// Currently, we do not use the ALERT pin feature. In the future, this could be a
	// useful overcurrent (i.e. short) detection feature.
	// If we were to, then we would possibly want to lower the current detection times
	// (and increase averaging to make up for the accuracy loss) to make response time
	// faster.

	Ok(())
}

#[inline] pub async fn get_power(
	i2c: &mut I2cDriver<'_>
) -> Result<PowerResponse, AnyhowError> {
	let mut voltage_raw = [0_u8; 2];
	i2c.write_read(
		power_sensor::ADDR,
		&[power_sensor::REG_VOLTAGE], voltage_raw.as_mut_slice(),
		TIMEOUT_1S).map_err(damn!("ADC voltage read failed"))?;

	let mut current_raw = [0_u8; 2];
	i2c.write_read(
		power_sensor::ADDR,
		&[power_sensor::REG_CURRENT], current_raw.as_mut_slice(),
		TIMEOUT_1S).map_err(damn!("ADC current read failed"))?;

	// Voltage LSB = 1.25mV
	// Current LSB = 1.25mA

	let voltage = (voltage_raw[0] as u16) << 8 | voltage_raw[1] as u16;
	let current = (current_raw[0] as u16) << 8 | current_raw[1] as u16;

	let mv = voltage as f32 * 1.25;
	let ma = current as f32 * 1.25;

	Ok(PowerResponse { mv, ma })
}