use crate::{get_time, prelude::*, tasks::{
	charter::{
		CharterState,
		Depth
	},
	i2c::I2cCommand,
}, TimeContainer};

use std::fmt::{
	Debug,
	Display,
	Error as FmtError,
	Formatter,
};

use futures::{
	channel::oneshot::channel,
	join
};
use crate::tasks::i2c::PowerResponse;
////////////////////////////////////////////////////////////////

const SNAPSHOT_INTERVAL_MS: u64 = 500;

#[derive(Debug, Clone)]
pub struct SystemStatus {
	pub depth: Depth,
	pub power_measurement: PowerResponse,
	pub charter_state: Option<CharterState>,

	pub timestamp: TimeContainer,
	pub create_log_entry: bool
}

impl SystemStatus {
	pub fn create_log_entry(self: &Self) -> bool { self.create_log_entry }
}

impl Display for SystemStatus {
	fn fmt(&self, fmt: &mut Formatter<'_>) -> Result<(), FmtError> {
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
	status_sender: SystemStatusSender<'_>,
	i2c_sender: I2cSender<'_>,
	mut charter_state_receiver: CharterStateReceiver<'_>,
) -> Never {
	sleep_ms!(SNAPSHOT_INTERVAL_MS);

	let mut counter = 0;
	loop {
		counter += 1;
		let create_log_entry = counter == 10;
		if create_log_entry {
			counter = 0;
		}

		// To make things more efficient, we can concurrently await all four asynchronous
		// transactions that need to take place (respectively sending and receiving the
		// request and response for each of the power and depth measurements
		let (power_measurement, depth) = {
			let (power_tx, power_rx) = channel();
			let (depth_tx, depth_rx) = channel();

			let (_, power, _, depth) = join!(
				i2c_sender.send(I2cCommand::GetPower { response: power_tx }),
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

		let status = SystemStatus {
			depth,
			power_measurement,
			charter_state,

			timestamp: get_time(),
			create_log_entry,
		};

		if create_log_entry { info!("{status}"); }

		status_sender.send(status);

		sleep_ms!(SNAPSHOT_INTERVAL_MS);
	}
}
