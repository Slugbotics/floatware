use crate::{
	prelude::*,
	tasks::charter::{AbortReason, CharterState}
};

use std::{
	error::Error,
	fmt::{Debug, Display, Formatter, Result as FmtResult},
};

////////////////////////////////////////////////////////////////////////////////

/// Potentially misleading name: this function doesn't stop a task; it is the task
/// that causes the float to shut down when needed.
pub async fn shutdown_task(
	shutdown_signal: &ShutdownSignal,
	charter_state_sender: CharterStateSender<'_>,
) -> Void {
	// Suspend until something sends a signal on the shutdown channel
	let shutdown_request = shutdown_signal.wait().await;

	if shutdown_request.go_to_surface {
		charter_state_sender.send(CharterState::Aborted { reason: AbortReason::Shutdown });
		// TODO: some kind of sleep call, or wait for a response, to ensure it gets done
	}

	// Exit with an error, breaking out of the `try_join!` in float-thread
	Err(AnyhowError::new(Shutdown(shutdown_request)))
}

////////////////////////////////////////////////////////////////////////////////

/// Error type to indicate a normal shutdown
pub struct Shutdown(pub ShutdownRequest);

impl Error for Shutdown {}

impl Debug for Shutdown {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult { f.write_str("Shutdown") }
}

impl Display for Shutdown {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult { f.write_str("Shutdown") }
}

#[derive(Debug)]
pub struct ShutdownRequest {
	pub originator: &'static str,
	pub go_to_surface: bool,
}
