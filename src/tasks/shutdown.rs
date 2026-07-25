use crate::signals::ShutdownSignal;

use std::{
	error::Error,
	fmt::{Debug, Display, Formatter},
	sync::Arc
};

use anyhow::Error as AnyhowError;

////////////////////////////////////////////////////////////////////////////////

/// Potentially misleading name: this function doesn't stop a task; it is the task
/// that causes the float to shut down when needed.
/// TODO: maybe add a delay or signal to other tasks that they should gracefully exit
/// TODO: maybe convert () to an enum representing how important it is we immediately shut down
pub async fn shutdown_task(
	shutdown_signal: Arc<ShutdownSignal>
) -> Result<(), AnyhowError> {
	// Suspend until something sends a signal on the shutdown channel
	shutdown_signal.wait().await;
	// And then exit, breaking out of the `try_join!`
	Err(AnyhowError::new(Shutdown))
}

////////////////////////////////////////////////////////////////////////////////

pub struct Shutdown;

impl Debug for Shutdown {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result { f.write_str("Shutdown") }
}

impl Display for Shutdown {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result { f.write_str("Shutdown") }
}

impl Error for Shutdown {}
