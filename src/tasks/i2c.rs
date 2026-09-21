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

pub mod pressure_sensor;
pub mod temp_sensor;
pub mod power_sensor;

use {
	power_sensor::*,
	pressure_sensor::*,
	temp_sensor::*
};

use crate::prelude::*;

use std::{
	sync::OnceLock,
	thread::{Builder as ThreadBuilder, JoinHandle}
};

use esp_idf_svc::{
	hal::{
		delay::TickType,
		gpio::{InputPin, OutputPin},
		i2c::{
			I2c,
			I2cConfig,
			I2cDriver
		},
		task::block_on,
		units::Hertz
	},
	sys::{
		xTaskGetCurrentTaskHandle,
		TaskHandle_t,
		TickType_t
	}
};

use futures::channel::oneshot::Sender as OneshotSender;

////////////////////////////////////////////////////////////////////////////////

/// TODO: document
///
macro_rules! make_i2c_commands {
    [$(
	$name:ident($getter:ident): $response:ident {
		$($response_fields:tt)*
	} @ $present_field:ident $(+ {
		$($status_fields:tt)*
	})?
	),+ $(,)?] => {
		pub enum I2cCommand {
			$(
				$name {
					response: OneshotSender<$response>
				},
			)+
		}
		impl I2cCommand {
			pub fn name(&self) -> &'static str {
				match self {
					$(
						Self::$name { .. } => stringify!($name),
					)+
				}
			}
			#[inline] async fn handle(
				self,
				i2c: &mut I2cDriver<'_>,
				device_status: &I2cDevicesStatus,
			) -> Result<(), AnyhowError> {
				Ok(match self {
					$(
						Self::$name { response } => if response.send(
							if device_status.$present_field {
								$getter(i2c).await?
							} else {
								Default::default()
							}
						).is_err() {
							// The code that holds the receiver has somehow stopped waiting properly.
							warn!(concat!("I2C ", stringify!($name), ": receiver dropped"));
						}
					)+
				})
			}
		}
		$(
			#[derive(Debug, Default, Copy, Clone)]
			pub struct $response {
				$($response_fields)*
			}
		)+
		#[derive(Default)]
		struct I2cDevicesStatus {
			$(
				$present_field: bool,
				$($($status_fields)*)*
			)*
		}
	};
}

make_i2c_commands![
	GetTH(get_temp_humidity): TempHumidityResponse {
		pub celsius: u8,
		/// A percentage.
		pub relative_humidity: u8
	} @ th_present,

	GetDepth(get_pressure): DepthResponse {
		pub depth: Depth,
	} @ pressure_present + {
		/// 14 bytes read from the MS5837 on startup
		pressure_prom: [u8; 14],
	},

	GetPower(get_power): PowerResponse {
		pub ma: f32,
		pub mv: f32,
	} @ power_sensor_present,
];

////////////////////////////////////////////////////////////////////////////////

static I2C_THREAD_HANDLE: OnceLock<usize> = OnceLock::new();

/// Using this in functions outside the float thread is probably not safe
#[inline] pub fn get_i2c_thread_handle() -> TaskHandle_t { *I2C_THREAD_HANDLE.get().unwrap() as _ }

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
		baudrate: Hertz(100_000),
		// timeout: Some(Duration::from_millis(100).into()), // probably excessive
		..Default::default()
	}).map_err(damn!("Failed to create i2c driver"))?;

	let handle = ThreadBuilder::new()
		.name("i2c-thread".to_string())
		.stack_size(64 * 1024)
		.spawn(move || {
			// This closure is the synchronous root of the i2c-thread.
			I2C_THREAD_HANDLE.set(not_unsafe! { xTaskGetCurrentTaskHandle() } as usize).unwrap(/* unreachable */);
			block_on(i2c_thread(driver, receiver))
		})
		.map_err(damn!("Failed to spawn i2c thread"))?;

	info!("Successfully initialized i2c thread");
	Ok(handle)
}

////////////////////////////////////////////////////////////////////////////////

pub const TIMEOUT_1S: TickType_t = TickType::new_millis(1000).0;

async fn i2c_thread<'a>(
	mut i2c: I2cDriver<'_>,
	rx: I2cReceiver<'_>
) -> Never {
	let status = reset_i2c_devices(&mut i2c).await;

	loop {
		let cmd = rx.receive().await;

		debug!("Received i2c command {}", cmd.name());

		// We need to let the handler take ownership of cmd,
		// so grab the name first in case of failure.
		let name = cmd.name();

		if let Err(error) = cmd.handle(&mut i2c, &status).await {
			warn!("Error processing I2C {name}: {error:#?}")
		}
	}
}

#[inline] async fn reset_i2c_devices(
	i2c: &mut I2cDriver<'_>,
) -> I2cDevicesStatus {
	let mut status: I2cDevicesStatus = Default::default();

	// Reset depth sensor
	status.th_present = reset_th_sensor(i2c)
		.map_err(|error| warn!("Failed to initialize T&H sensor: {:#?}", error))
		.is_ok();
	// No delay because we have a lot of other things to do first before processing I2C commands

	status.pressure_present = match pressure_sensor::pressure_sensor_reset_and_read_prom(i2c) {
		Ok(prom) => {
			status.pressure_prom = prom;
			true
		}
		Err(error) => {
			warn!("Failed to initialize pressure sensor: {:#?}", error);
			false
		}
	};

	status.power_sensor_present = reset_ina260(i2c).await
		.map_err(|error| warn!("Failed to initialize ADC: {:#?}", error))
		.is_ok();

	status
}

////////////////////////////////////////////////////////////////////////////////

