#![feature(
	io_error_too_many_open_files,
	io_error_input_output_error,
	const_trait_impl,
	map_try_insert,
)]

mod floatware;
mod tasks;
mod signals;
mod prelude;
mod debugging;
mod config;
mod timekeeping;
mod gpio;

use crate::{
	prelude::*,
};

use std::{
	thread,
	sync::OnceLock
};

use esp_idf_svc::{
	hal::task::block_on,
	sys::{xTaskGetCurrentTaskHandle, TaskHandle_t}
};

////////////////////////////////////////////////////////////////////////////////

/// The default entry point. We immediately spawn and join a secondary
/// "float-thread"  where everything takes place. We do this for a few reasons:
/// - Priority: the main thread has a fairly low priority*, and this gives our code
///   a higher priority to prevent being potentially stepped on by library threads
/// - Stack size: Makes it easier to configure float thread stack size
/// - There is no third reason.
///
/// All the "actual" initialization code lives in [floatware::float_thread].
///
/// *See https://github.com/esp-rs/esp-idf-svc/blob/master/examples/tls_async.rs#L49
fn main() {
	timekeeping::register_boot_instant();

	// It is necessary to call this function once. Otherwise, some patches to the runtime
	// implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
	esp_idf_svc::sys::link_patches();

	// Bind the log crate to the ESP Logging facilities
	esp_idf_svc::log::EspLogger::initialize_default();

	match thread::Builder::new()
		.name("float-thread".to_string())
		.stack_size(16 * 1024)
		.spawn(move || {
			// This closure is the synchronous root of the float-thread.
			FLOAT_THREAD_HANDLE.set(/* not */ not_unsafe! { xTaskGetCurrentTaskHandle() } as usize).unwrap(/* unreachable */);
			block_on(floatware::float_thread())
		}) {
		Err(spawning_err) => error!("FATAL: Unable to spawn float thread: {spawning_err:#?}"),
		Ok(handle) => match handle.join() {
			Err(thread_panic) => error!("FATAL: Float thread panicked: {thread_panic:#?}"),
			// The return value of `float_thread()` is then returned inside Ok():
			Ok(Err(error)) => error!("FATAL: Float thread returned error: {error:#?}"),
			Ok(Ok(())) => info!("Float thread returned normally. Shutting down."),
		},
	};
}

pub static FLOAT_THREAD_HANDLE: OnceLock<usize> = OnceLock::new();

/// Using this in functions outside the float thread is probably not safe
#[inline] pub fn get_float_thread_handle() -> TaskHandle_t { *FLOAT_THREAD_HANDLE.get().unwrap() as _ }