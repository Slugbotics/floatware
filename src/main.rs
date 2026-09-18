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

use crate::prelude::*;

use std::{
	thread,
	time::{
		Instant,
		Duration
	},
	fmt::{
		Debug,
		Formatter,
		Display
	}
};

use esp_idf_svc::hal::task::block_on;
use time::error::Format;
use time::macros::format_description;
use time::Timestamp;

////////////////////////////////////////////////////////////////////////////////

/// The default entry point. We immediately spawn and join a secondary
/// "float-thread"  where everything takes place. We do this for a few reasons:
/// - Priority: the main thread has a fairly low priority*, and this gives our code
///   a higher priority to prevent being potentially stepped on by library threads
/// - Stack size: I'm not sure how to configure the main thread stack size but I'm
///   pretty sure that it's not as simple as it is here.
/// - There is no third reason.
///
/// All the "actual" initialization code lives in [floatware::float_thread].
///
/// *See https://github.com/esp-rs/esp-idf-svc/blob/master/examples/tls_async.rs#L49
///
/// TODO: figure out how much stack memory is needed for the thread. The current is arbitrary.
fn main() {
	unsafe { BOOT_TIME = Some(Instant::now()); }

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

////////////////////////////////////////////////////////////////////////////////

/// Set once on boot. Always safe to unwrap.
static mut BOOT_TIME: Option<Instant> = None;

/// Requires synchronization data sent from the controlling computer via http, so
/// may not be populated.
static mut BOOT_TIMESTAMP: Option<Timestamp> = None;

/// Return either the time since boot, or the current Unix time.
pub fn get_time() -> TimeContainer {
	unsafe {
		BOOT_TIMESTAMP.map(|ts| TimeContainer::Timestamp(ts))
			.unwrap_or_else(|| (Instant::now() - BOOT_TIME.unwrap()).into())
	}
}

pub fn set_boot_time(now: Timestamp) {
	unsafe {
		BOOT_TIMESTAMP = Some(now);
	}
}

#[derive(Clone)]
pub enum TimeContainer {
	SinceBoot(Duration),
	Timestamp(Timestamp),
}

impl From<Timestamp> for TimeContainer {
	fn from(value: Timestamp) -> Self { Self::Timestamp(value) }
}

impl From<Duration> for TimeContainer {
	fn from(value: Duration) -> Self { Self::SinceBoot(value) }
}

impl Display for TimeContainer {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::SinceBoot(duration) =>
				f.write_fmt(format_args!("{}.{:06}", duration.as_secs(), duration.subsec_micros())),
			Self::Timestamp(timestamp) => {
				let res = timestamp.format(format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond]"));
				f.write_str(match &res {
					Ok(s) => s.as_str(),
					Err(_) => "<time format error>"
				})
			}
		}
	}
}

impl Debug for TimeContainer {
	fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::SinceBoot(duration) => duration.fmt(f),
			Self::Timestamp(timestamp) => Debug::fmt(timestamp, f),
		}
	}
}
