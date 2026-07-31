//! The current esp-idf implementation of I2C is fully blocking, so we actually
//! create a separate I2C worker thread to handle the communication. The float
//! thread sends in request objects which have an embedded single-use response
//! channel, and this thread (eventually) replies with data. Of course, the
//! originating code can `await` the response so the float thread can do other
//! things in the meantime.
//!
//! To be clear, when I say that the current I2C driver is "blocking", I don't mean
//! that it actually halts the CPU for the entire duration of the transaction, just
//! that the I2C-calling thread cannot continue to do other work until the
//! transaction completes. The thread is blocked but other threads are able to run.
//! Hence, why we make all the magic sauce happen in this thread.

use crate::{
	prelude::*,
	tasks::charter::Depth
};

use std::{
	thread::{Builder as ThreadBuilder, JoinHandle}
};

use esp_idf_svc::{
	hal::{
		gpio::{InputPin, OutputPin},
		i2c::{
			I2c,
			I2cConfig,
			I2cDriver
		},
		task::block_on,
		units::Hertz,
		delay::TickType
	},
	sys::TickType_t,
};

use cfor::cfor;

use futures::channel::oneshot::Sender as OneshotSender;

////////////////////////////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct TempHumidityResponse {
	pub celsius: u8,
	/// A percentage.
	pub relative_humidity: u8
}

impl Default for TempHumidityResponse {
	fn default() -> Self {
		TempHumidityResponse {
			celsius: 0,
			relative_humidity: 0,
		}
	}
}

#[derive(Debug)]
pub struct DepthResponse {
	pub depth: Depth,
}

impl Default for DepthResponse {
	fn default() -> Self {
		DepthResponse {
			depth: 0.
		}
	}
}

pub enum I2cCommand {
	GetTH {
		response: OneshotSender<TempHumidityResponse>,
	},
	GetDepth {
		response: OneshotSender<DepthResponse>
	}
}

impl I2cCommand {
	fn name(&self) -> &'static str {
		match self {
			I2cCommand::GetTH { .. } => "T&H",
			I2cCommand::GetDepth { .. } => "Depth"
		}
	}
}

////////////////////////////////////////////////////////////////////////////////

/// Spawn a separate FreeRTOS thread for the I2C handler task, then return a handle
/// to it.
///
/// TODO: stack size
pub fn initialize_i2c_thread(
	i2c: impl I2c + 'static,
	sda: impl InputPin + OutputPin + 'static,
	scl: impl InputPin + OutputPin + 'static,
	receiver: I2cReceiver<'static>,
) -> Result<JoinHandle<Never>, AnyhowError> {
	let driver = I2cDriver::new(i2c, sda, scl, &I2cConfig {
		baudrate: Hertz(100_000), // pressure sensor max is 400 kHz
		// timeout: Some(Duration::from_millis(100).into()), // probably excessive
		..Default::default()
	}).map_err(damn!("Failed to create i2c driver"))?;

	let handle = ThreadBuilder::new()
		.name("i2c-thread".to_string())
		.stack_size(64 * 1024)
		.spawn(move || {
			// This closure is the synchronous root of the i2c-thread.
			block_on(i2c_thread(driver, receiver))
		})
		.map_err(damn!("Failed to spawn i2c thread"))?;

	info!("Successfully initialized i2c thread");
	Ok(handle)
}

////////////////////////////////////////////////////////////////////////////////

/// SHTC3
mod th_sensor {
	pub const ADDR: u8 = 0x70;

	pub const CMD_RESET: [u8; 2] = [0x80, 0x5D];
	pub const CMD_WAKE:  [u8; 2] = [0x35, 0x17];
	pub const CMD_MEAS:  [u8; 2] = [0x78, 0x66];
	pub const CMD_SLEEP: [u8; 2] = [0xB0, 0x98];
}

/// MS5837
mod p_sensor {
	pub const ADDR: u8 = 0x76;

	pub const CMD_RESET: u8 = 0x1E;
	/// Must be ORed with the offset (0x00 -- 0x0C, even bytes only). Returns 2 bytes.
	pub const CMD_READ_PROM: u8 = 0xA0;

	pub const CMD_READ_ADC: u8 = 0x00;

	pub const CMD_CONVERT_D1: u8 = 0x40;
	pub const CMD_CONVERT_D2: u8 = 0x50;
}

const TIMEOUT_1S: TickType_t = TickType::new_millis(1000).0;

#[derive(Default)]
struct I2cDevicesStatus {
	th_present: bool,
	pressure_present: bool,
	pressure_prom: [u8; 14],
}

async fn i2c_thread<'a>(
	mut i2c: I2cDriver<'_>,
	rx: I2cReceiver<'_>
) -> Never {
	let i2c_init_data = reset_i2c_devices(&mut i2c);

	let mut status: I2cDevicesStatus = Default::default();

	status.th_present = match i2c_init_data.th_init {
		Ok(()) => true,
		Err(error) => {
			warn!("Failed to initialize T&H sensor: {:#?}", error);
			false
		}
	};

	status.pressure_present = match i2c_init_data.pressure_init {
		Ok(prom) => {
			status.pressure_prom = prom;
			true
		}
		Err(error) => {
			warn!("Failed to initialize pressure sensor: {:#?}", error);
			false
		}
	};

	loop {
		let cmd = rx.receive().await;

		debug!("Received i2c command {}", cmd.name());

		// We need to let the handler take ownership of cmd,
		// so grab the name first in case of failure.
		let name = cmd.name();

		if let Err(error) = handle_i2c_command(&mut i2c, cmd, &status).await {
			warn!("Error processing I2C {name}: {error:#?}")
		}
	}
}

struct I2cResetStatus {
	th_init: Result<(), AnyhowError>,
	pressure_init: Result<[u8; 14], AnyhowError>,
}

#[inline] fn reset_i2c_devices(
	i2c: &mut I2cDriver<'_>,
) -> I2cResetStatus {
	// Reset depth sensor
	let th_init = i2c.write(
		th_sensor::ADDR, &th_sensor::CMD_RESET, TIMEOUT_1S
	).map_err(damn!("T&H sensor reset failed"));

	// No delay because we have a lot of other things to do first before processing I2C commands

	let pressure_init = pressure_sensor_reset_and_read_prom(i2c);

	I2cResetStatus {
		th_init,
		pressure_init,
	}
}

#[inline] fn pressure_sensor_reset_and_read_prom(i2c: &mut I2cDriver<'_>) -> Result<[u8; 14], AnyhowError> {
	i2c.write(
		p_sensor::ADDR, &[p_sensor::CMD_RESET], TIMEOUT_1S
	).map_err(damn!("Pressure sensor reset failed"))?;

	// TODO: delay?

	// Read PROM data (ds. page 8)
	let mut prom = [0_u8; 14];

	for off in (0x00 ..= 0x0C).step_by(2) {
		i2c.write(p_sensor::ADDR, &[p_sensor::CMD_READ_PROM | off as u8], TIMEOUT_1S)
			.map_err(damn!("Pressure sensor PROM request failed at offset 0x{:02X}", off))?;
		i2c.read(p_sensor::ADDR, &mut prom[off .. off+2], TIMEOUT_1S)
			.map_err(damn!("Pressure sensor PROM read failed at offset 0x{:02X}", off))?;
	}

	Ok(prom)
}

/// Pulled out into a separate function to make error handling easier with `?`.
///
/// Note that `#[inline]` doesn't force inlining, but is just a suggestion to the
/// compiler.
#[inline] async fn handle_i2c_command(
	i2c: &mut I2cDriver<'_>,
	cmd: I2cCommand,
	device_status: &I2cDevicesStatus,
) -> Result<(), AnyhowError> {
	debug!("Handling i2c command {}", cmd.name());

	match cmd {
		I2cCommand::GetTH { response } => Ok(
			if device_status.th_present {
				let packet = get_temp_humidity(i2c).await?;

				if response.send(packet).is_err() {
					// The code that holds the receiver has somehow stopped waiting properly.
					warn!("I2C T&H: receiver dropped");
				}
			} else {
				let _ = response.send(Default::default());
			}
		),
		I2cCommand::GetDepth { response } => Ok(
			if device_status.pressure_present {
				let packet = get_pressure(i2c).await?;

				if response.send(packet).is_err() {
					warn!("I2C Depth: receiver dropped");
				}
			} else {
				let _ = response.send(Default::default());
			}
		)
	}
}

////////////////////////////////////////////////////////////////////////////////

#[inline] async fn get_temp_humidity(
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

#[inline] async fn get_pressure(
	i2c: &mut I2cDriver<'_>,
) -> Result<DepthResponse, AnyhowError> {
	//! TODO
	Ok(DepthResponse { depth: 0. })
}

////////////////////////////////////////////////////////////////////////////////

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
