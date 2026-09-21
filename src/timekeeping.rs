use std::{
	fmt::{Debug, Display, Error as FmtError, Formatter},
	sync::OnceLock,
	time::{Duration, Instant}
};

use time::{
	macros::format_description,
	Timestamp
};

////////////////////////////////////////////////////////////////////////////////

/// Set once on boot. Always safe to unwrap.
static BOOT_TIME: OnceLock<Instant> = OnceLock::new();
/// Requires synchronization data sent from the controlling computer via http, so
/// may not be populated.
static BOOT_TIMESTAMP: OnceLock<Timestamp> = OnceLock::new();

#[inline(always)] pub(super) fn register_boot_instant() {
	BOOT_TIME.set(Instant::now()).unwrap(/* unreachable */);
}

#[inline] pub fn get_on_duration() -> Duration {
	BOOT_TIME.get().unwrap(/* unreachable */).elapsed()
}

/// Return either the time since boot, or the current Unix time.
pub fn get_time() -> TimeContainer {
	BOOT_TIMESTAMP.get()
		.map(|ts| (*ts + get_on_duration()).into())
		.unwrap_or_else(|| get_on_duration().into())
}

/// Return false if the time has previously been set.
#[must_use] pub fn set_boot_timestamp(now: Timestamp) -> bool {
	BOOT_TIMESTAMP.set(now).is_ok()
}

////////////////////////////////////////////////////////////////////////////////

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
	fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
		match self {
			Self::SinceBoot(duration) =>
				f.write_fmt(format_args!("{}.{:03}", duration.as_secs(), duration.subsec_millis())),
			Self::Timestamp(timestamp) => {
				f.write_str(match &timestamp.format(format_description!(
					"[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]"
				)) {
					Ok(s) => s.as_str(),
					Err(_) => "<time format error>"
				})
			}
		}
	}
}

impl Debug for TimeContainer {
	fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), FmtError> {
		match self {
			Self::SinceBoot(duration) => duration.fmt(f),
			Self::Timestamp(timestamp) => Debug::fmt(timestamp, f),
		}
	}
}
