mod floatware;
mod tasks;
mod signals;

use std::thread;

use esp_idf_svc::hal::task::block_on;

use log::{error, info};

use anyhow::Error as AnyhowError;

////////////////////////////////////////////////////////////////////////////////

/// The default entry point. We immediately spawn and join a secondary "app-thread"
/// where everything takes place. We do this for a few reasons:
/// - Priority: the main thread has a fairly low priority*, and this gives our code
///   a higher priority to prevent being potentially stepped on by library threads
/// - Stack size: I'm not sure how to configure the main thread stack size but I'm
///   pretty sure that it's not as simple as it is here.
/// - There is no third reason.
///
/// All the "actual" initialization code lives in [floatware::initialize].
///
/// *See https://github.com/esp-rs/esp-idf-svc/blob/master/examples/tls_async.rs#L49
///
/// TODO: figure out how much stack memory is needed for the thread. The current is arbitrary.
fn main() {
	// It is necessary to call this function once. Otherwise, some patches to the runtime
	// implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
	esp_idf_svc::sys::link_patches();

	// Bind the log crate to the ESP Logging facilities
	esp_idf_svc::log::EspLogger::initialize_default();

	match thread::Builder::new()
		.name("app-thread".to_string())
		.stack_size(64 * 1024)
		.spawn(execute_app_thread) {
		Err(spawning_err) => error!("FATAL: Unable to spawn app thread: {spawning_err:#}"),
		Ok(handle) => match handle.join() {
			Err(thread_panic) => error!("FATAL: App thread panicked: {thread_panic:?}"),
			// The return value of initialize is then returned inside Ok():
			Ok(Err(error)) => error!("FATAL: App thread returned error: {error:#}"),
			Ok(Ok(())) => info!("App thread returned normally. Shutting down."),
		},
	};
}

/// We need a synchronous root of the mostly asynchronous app thread.
/// We get to choose our future executor, but as I understand it, the builtin
/// [block_on] is perfectly good.
fn execute_app_thread() -> Result<(), AnyhowError> {
	block_on(floatware::initialize())
}
