use crate::{
	prelude::*
};

use std::{
	time::{
		Duration,
		Instant
	},
	fmt::{Display, Formatter, Result as FmtResult}
};

use embassy_time::WithTimeout;
use crate::tasks::stepper_controller::StepperState;
////////////////////////////////////////////////////////////////////////////////

/// TODO based on I2C depth sensor driver code
pub type Depth = f64;
/// First argument is the amount of time after starting the dive that we should be
/// at this [Depth]. The duration that we are at that depth is the following entry's
/// [Duration] minus this one.
/// 
/// WARNING: durations of more than 584542 years may lead to an error in the sleep code.
pub type DepthEntry = (Duration, Depth);

/// Maps the time after starting the descent, to a target depth.
/// Note that the last entry will, for implementation reasons, always be effectively
/// skipped, so it should be a depth of 0 so that the last non-surface entry is not
/// skipped.
pub type Charter = Vec<DepthEntry>;

/// The current state of the float with respect to a dive.
#[derive(Clone, Debug)]
pub enum CharterState {
	StartRequested,
	InProgress {
		/// The current depth entry index
		charter_index: usize,
		target_depth: Depth,
		/// Used for the status LED to figure out how far through the charter we are
		charter_size: usize,
	},
	Completed,
	Aborted {
		reason: AbortReason
	}
}

impl Display for CharterState {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
		match self {
			Self::StartRequested => f.write_str("StartRequested"),
			Self::InProgress { charter_index, target_depth, charter_size } =>f.write_fmt(format_args!("InProgress {{ {charter_index}/{charter_size}: {target_depth} }}")),
			Self::Completed => f.write_str("Completed"),
			Self::Aborted { reason } => f.write_fmt(format_args!("Aborted {{ {reason} }}"))
		}
	}
}

#[derive(Clone, Debug)]
pub enum AbortReason {
	CharterStateError { offending_charter_index: usize },
	Shutdown,
}

impl Display for AbortReason {
	fn fmt<'a>(&self, f: &mut Formatter<'_>) -> FmtResult {
		match self {
			AbortReason::CharterStateError { offending_charter_index } => {
				f.write_fmt(format_args!("Invalid charter state: InProgress({offending_charter_index}) at beginning of depth_target_update_task"))
			}
			AbortReason::Shutdown => f.write_str("Shutdown")
		}
	}
}

////////////////////////////////////////////////////////////////////////////////

/// Initialized with a charter to follow. When [CharterState::StartRequested] is
/// received, the task begins going through the charter, sending new states on the
/// channel.
///
/// Signal to begin the charter
/// Current charter
/// Output: needs to be sent to depth control service
pub async fn depth_target_update_task(
	mut charter_state_receiver: CharterStateReceiver<'_>,
	charter_state_sender: CharterStateSender<'_>,
	charter: &Charter
) -> Never {
	'enclosing: loop {
		match charter_state_receiver.changed().await {
			CharterState::StartRequested => {},
			CharterState::InProgress { charter_index, .. } => {
				charter_state_sender.send(CharterState::Aborted {
					reason: AbortReason::CharterStateError { offending_charter_index: charter_index }
				});
				continue;
			}
			CharterState::Aborted { reason } => {
				error!("Charter aborted: {}", reason);
				continue;
			}
			CharterState::Completed => continue // Nothing to do
		};

		let charter_size = charter.len();

		// Find charter base beginning time
		let start = Instant::now();

		for (
			charter_index, (target_start_time, target_depth)
		) in charter.iter().enumerate() {
			// Determine how much time we need to wait until `start_time` has passed since `start`
			let time_since_start = start.elapsed();

			// We need to wait another (start_time - time_since_start)
			if sleep_and_check_aborted(
				&mut charter_state_receiver,
				target_start_time.saturating_sub(time_since_start)
			).await.is_some() {
				info!("Depth target task: aborting charter");
				// We are aborting, stop setting new charter states
				continue 'enclosing;
				// Depth control has also received this signal so there's no need to do anything
			}

			info!("New target: {target_depth:?}, {} seconds after start",
				target_start_time.as_secs());

			// Because the esp32c3 is single-core, we avoid a race condition, wherein an Aborted
			// value is inserted into the receiver between where we check for one above and where
			// we overwrite the current state. If it was possible for multiple tasks to execute
			// simultaneously, we would have to defend against it. But it is not possible.
			// However, different FreeRTOS tasks (i.e. threads) can be preemptively time-sliced,
			// so this would be a concern if it was possible for an Aborted value to be inserted
			// into the sender from a different thread. However, it is not currently possible for
			// that to happen, given the current architecture of the firmware.

			charter_state_sender.send(CharterState::InProgress {
				charter_index, target_depth: *target_depth, charter_size
			});
		}

		info!("Finished charter. Returning to surface.");

		charter_state_sender.send(CharterState::Completed);
	}
}

/// Reads the current charter state, and then sends commands to the stepper to bring
/// the current depth closer to the target.
pub async fn depth_control_task(
	mut charter_state_receiver: CharterStateReceiver<'_>,
	mut status_receiver: SystemStatusReceiver<'_>,
	// i2c_sender: I2cSender<'_>,
	stepper_state_channel: &StepperStateSignal
) -> Void {
	/// this will inevitably become a closure when implemented due to the need to capture stepper
	async fn go_to_surface() {
		//! TODO
		info!("Going to surface");
	}

	// These are the only states we care about
	let charter_state_predicate = |state: &CharterState|
		matches!(state, CharterState::InProgress {..} | CharterState::Completed | CharterState::Aborted { .. });

	// If we are in the middle of diving, then we need to continually **get** the value,
	// because we need to continually loop to update the stepper to maintain buoyancy.
	// But if we are not diving, then we do not need to continually get the value; we can just
	// *wait* for it to **change**.
	let mut dive_in_progress = false;

	loop {
		let state = if dive_in_progress {
			charter_state_receiver.get_and(charter_state_predicate).await
		} else {
			charter_state_receiver.changed_and(charter_state_predicate).await
		};

		info!("Depth control: got state: {:?}", state);

		match state {
			CharterState::InProgress { target_depth, .. } => {
				dive_in_progress = true;

				let current_depth = status_receiver.get().await.depth;

				stepper_state_channel.signal(
					if current_depth > target_depth {
						StepperState::WeAreTooLow
					} else {
						StepperState::WeAreTooHigh
					}
				);

				// TODO: adjust stepper to get closer to target, then sleep or something
			}
			CharterState::Aborted { reason: AbortReason::Shutdown } => {
				go_to_surface().await;

				// Note: there may be a possible race condition here, where a shutdown is called,
				// but this task is busy and doesn't handle it, and in that time, the depth update
				// task overwrites it. But since this isn't actually multithreaded, I don't *think*
				// that can happen?
				// In any case, if we sleep in this function, that can absolutely be a problem,
				// so any sleep calls should use sleep_and_check_aborted

				return Ok(()); // The depth update task may send more states which we want to ignore
			}
			CharterState::Completed | CharterState::Aborted { .. } => {
				dive_in_progress = false;

				go_to_surface().await;
			}
			_ => unreachable!()
		}
	}
}

/// Instead of just sleeping, we instead create a timeout future that waits for the
/// charter state to become Aborted, timing out after the amount of sleep time needed.
/// In almost all cases, the timeout will occur, but if something triggers an external
/// abort, we will see that instead, and will exit.
///
/// Note: this function will panic if the supplied [Duration] exceeds 584542 years.
#[inline] async fn sleep_and_check_aborted(
	charter_state_receiver: &mut CharterStateReceiver<'_>,
	duration: Duration,
) -> Option<AbortReason> {
	if let Ok(CharterState::Aborted { reason }) = charter_state_receiver.changed_and(
		|state| matches!(state, CharterState::Aborted { .. })
	).with_timeout(
		duration.try_into().unwrap() // Will fail if duration > 584542 years
	).await {
		error!("Charter aborted: {reason}");
		Some(reason)
	} else { None }
}
