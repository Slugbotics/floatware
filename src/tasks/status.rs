use crate::{prelude::*, get_time, tasks::{
	charter::{
		Depth,
		CharterState
	},
	power_measurement::PowerMeasurement,
	i2c::I2cCommand,
	led::LEDState
}, TimeContainer, tasks};

use std::fmt::{Debug, Display};
use std::slice;
use futures::{
	channel::oneshot::channel,
	join
};
use smart_leds_trait::RGB8;
use tasks::led;
////////////////////////////////////////////////////////////////

const SNAPSHOT_INTERVAL_MS: u64 = 100;

#[derive(Debug, Clone)]
pub struct SystemStatus {
	pub depth: Depth,
	pub power_measurement: PowerMeasurement,
	pub charter_state: Option<CharterState>,

	pub timestamp: TimeContainer,
	pub create_log_entry: bool
}

impl SystemStatus {
	pub fn create_log_entry(self: &Self) -> bool { self.create_log_entry }
}

impl Display for SystemStatus {
	fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
		fmt.write_fmt(format_args!(
			"[{}] depth: {}; voltage: {}; current: {}; charter state: ",
			self.timestamp, self.depth, self.power_measurement.voltage,
			self.power_measurement.current))?;

		if let Some(ref state) = self.charter_state {
			Display::fmt(&state, fmt)
		} else {
			fmt.write_str("None")
		}
	}
}

////////////////////////////////////////////////////////////////

/// Creates intermittent system status snapshots.
/// Tasks can read their channel to see the current system state without having to
/// query the hardware themselves.
pub async fn status_publishing_task(
	power_measurement_request_sender: PowerMeasurementRequestSender<'_>,
	status_sender: StatusSender<'_>,
	i2c_sender: I2cSender<'_>,
	mut charter_state_receiver: CharterStateReceiver<'_>,
	led_signal: &LedStateSignal,
) -> Never {
	let mut counter = 0u8;
	loop {
		counter += 1;
		let create_log_entry = counter == 10;
		if create_log_entry {
			counter = 0;
		}

		// To make things more efficient, we can concurrently await all four asynchronous
		// transactions that need to take place (respectively sending and receiving the request and
		// response for each of the power and depth measurements
		let (power_measurement, depth) = {
			let (power_tx, power_rx) = channel();
			let (depth_tx, depth_rx) = channel();

			let (_, power, _, depth) = join!(
				power_measurement_request_sender.send(power_tx),
				power_rx,
				i2c_sender.send(I2cCommand::GetDepth { response: depth_tx }),
				depth_rx,
			);

			(
				power.unwrap_or_else(|_| {
					warn!("Status publishing task: power measurement: sender dropped");
					Default::default()
				}),
				depth.unwrap_or_else(|_| {
					warn!("Status publishing task: depth measurement: sender dropped");
					Default::default()
				}).depth
			)
		};

		let charter_state = charter_state_receiver.try_get();

		// The LED can be set to blink in a sequence of colors, so different systems can
		// convey their statuses simultaneously.
		// Currently, though, the only thing that does is the charter.
		let charter_led = match &charter_state {
			None => led::NONE,
			Some(state) => match state {
				CharterState::StartRequested => led::BLUE,
				CharterState::InProgress {
					charter_index, target_depth: _, charter_size
				} => { // Fade from blue to green as the charter is completed
					let progress = (charter_index * 255 / charter_size) as u8; // 0 to 255
					RGB8::new(0, progress, 255 - progress)
				},
				CharterState::Completed => led::GREEN,
				CharterState::Aborted { .. } => led::ORANGE,
			}
		};

		let led_state = if charter_led == led::NONE {
			LEDState::new(&[led::NONE, led::DIM])? // Heartbeat of sorts if no charter
		} else {
			LEDState::new(slice::from_ref(&charter_led))?
		};

		led_signal.signal(led_state);

		status_sender.send(SystemStatus {
			depth,
			power_measurement,
			charter_state,

			timestamp: get_time(),
			create_log_entry,
		});

		sleep_ms!(SNAPSHOT_INTERVAL_MS);
	}
}
