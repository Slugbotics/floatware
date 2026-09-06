#![allow(unused_imports)]

use std::{
	convert::Infallible,
	time::Duration
};

use esp_idf_svc::timer::EspAsyncTimer;

pub(crate) use {
	crate::{
		signals::*,
		tasks::*,
		damn, sleep, sleep_ms, ret_err, SD_CARD_NAME, sd
	},
	anyhow::Error as AnyhowError,
	log::{debug, info, warn, error},
};

////////////////////////////////////////////////////////////////////////////////

/// Okay, maybe I write too much Java.
pub(crate) type Void = Result<(), AnyhowError>;

/// Return type indicating that a task will never exit normally, instead looping
/// forever until it errors or is externally terminated.
pub(crate) type Never = Result<Infallible, AnyhowError>;

////////////////////////////////////////////////////////////////////////////////

/// Creates a closure that generates an error message and then returns the argument
/// wrapped inside an [AnyhowError]. Intended for use with [Result::map_err].
#[macro_export] macro_rules! damn {
	($($args:tt)+) => {
		|err__| {
			error!($($args)+);
			AnyhowError::from(err__)
		}
	};
	() => { AnyhowError::from };
}

/// Sleeps for the given [Duration] using [embassy_time]'s builtin tools. If
/// provided an [EspAsyncTimer], it will sleep using that instead. But the
/// former is probably preferred.
/// Can only be called inside an `async fn` returning `Result<_, AnyhowError>`.
#[macro_export] macro_rules! sleep {
	($timer:expr, $duration:expr) => {
		($timer).after($duration).await
			.map_err(damn!(concat!("Timer wait failure at ", file!(), ":", line!())))?
	};
	($duration:expr) => {
		embassy_time::Timer::after($duration).await
	};
 }

/// Sleeps for the given duration using [embassy_time]'s builtin tools. If provided
/// an [EspAsyncTimer], it will sleep using that instead. But the former is probably
/// preferred.
/// Can only be called inside an `async fn` returning `Result<_, AnyhowError>`.
#[macro_export] macro_rules! sleep_ms {
	($timer:expr, $ms:expr) => {
		sleep!($timer, std::time::Duration::from_millis($ms as u64))
	};
	($ms:expr) => {
		embassy_time::Timer::after_millis($ms).await
	};
 }

#[macro_export] macro_rules! ret_err {
     ($str:literal) => {
		 return Err(AnyhowError::msg($str))
	 };
 }

/// Workaround to make compile time string constants not be stupid inside of macros.
#[allow(non_snake_case)]
#[macro_export]
macro_rules! SD_CARD_NAME {
    () => { "sd" };
}

/// Create a full path, with the SD card's mountpoint, for the provided file path.
#[macro_export] macro_rules! sd {
    ($path:literal) => {
		concat!("/", SD_CARD_NAME!(), "/", $path)
	};
	($path:expr) => {
		format!(concat!("/", SD_CARD_NAME!(), "/", "{}"), $path)
	}
}
