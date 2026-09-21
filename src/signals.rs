use crate::{
	prelude::*,
	tasks::{
		i2c::I2cCommand,
		charter::CharterState,
		led::LedState,
		stepper_controller::{
			StepperState,
		},
		shutdown::ShutdownRequest,
		status::SystemStatus,
	}
};

use esp_idf_svc::hal::task::embassy_sync::EspRawMutex;

use embassy_sync::{
	signal::Signal,
	channel::{
		Channel,
		Sender as ChannelSender,
		Receiver as ChannelReceiver,
	},
	watch::{
		Watch,
		Sender as WatchSender,
		Receiver as WatchReceiver,
	},
	blocking_mutex::raw::{
		NoopRawMutex,
		CriticalSectionRawMutex
	}
};

use smart_leds_trait::RGB8;

////////////////////////////////////////////////////////////////////////////////

/// Constructs a set of three [Watch] types with the given (or assumed) name: a
/// normal (or "unsplit") watch, a receiver, and a sender.
macro_rules! build_watch {
    ($(#[$doc:meta])* $prefix:ident, $slot:ty, $mutex:ty, $recv_count:expr) => { ::paste::paste! {
		$(#[$doc])* pub(crate) type [<$prefix UnsplitWatch>] = Watch        <    $mutex, $slot, $recv_count>;
		$(#[$doc])* pub(crate) type [<$prefix Sender>]  <'a> = WatchSender  <'a, $mutex, $slot, $recv_count>;
		$(#[$doc])* pub(crate) type [<$prefix Receiver>]<'a> = WatchReceiver<'a, $mutex, $slot, $recv_count>;
	}};
	($(#[$doc:meta])* $slot:ty, $mutex:ty, $recv_count:expr) => {
		::paste::paste! {
			build_watch!(
				$(#[$doc])*
				[<$slot>], $slot, $mutex, $recv_count);
		}
	};
}

/// Similar as above, but for a channel. Note that the number is the channel buffer
/// queue depth; channels have no receiver limit.
macro_rules! build_channel {
    ($(#[$doc:meta])* $prefix:ident, $slot:ty, $mutex:ty, $buf_size:expr) => { ::paste::paste! {
		$(#[$doc])* pub(crate) type [<$prefix UnsplitChannel>] = Channel        <    $mutex, $slot, $buf_size>;
		$(#[$doc])* pub(crate) type [<$prefix Sender>]  <'a>   = ChannelSender  <'a, $mutex, $slot, $buf_size>;
		$(#[$doc])* pub(crate) type [<$prefix Receiver>]<'a>   = ChannelReceiver<'a, $mutex, $slot, $buf_size>;
	}};
}

/// Only safe for signals used within a single FreeRTOS task.
type SingleTaskMutex = NoopRawMutex;
/// Acceptable for signals accessed by multiple FreeRTOS tasks, but not interrupts.
type MultiTaskMutex = EspRawMutex;
/// Safe to use in interrupts and all places.
type IsrMutex = CriticalSectionRawMutex;

////////////////////////////////////////////////////////////////////////////////

/// Signal that tells the float-thread to exit. Written from the HTTP `/shutdown`.
pub(crate) type ShutdownSignal = Signal<MultiTaskMutex, ShutdownRequest>;

/// Used to send commands to the stepper motor. Only used on float thread.
pub(crate) type StepperStateSignal = Signal<SingleTaskMutex, StepperState>;

/// Used to send LED color information between tasks. Only used on float thread.
pub(crate) type LedColorSignal = Signal<SingleTaskMutex, RGB8>;

/// Used to set the current LED cycle. Only used on float thread.
pub(crate) type LedStateSignal = Signal<SingleTaskMutex, LedState>;

build_watch!(
/// Keeps track of the current [CharterState]; what we are currently doing on the
/// dive. Empty means no dive has been started yet.
///
/// Written by HTTP `/start_dive`.
	CharterState, MultiTaskMutex, 3
);

build_watch!(
/// Written from HTTP thread
	SystemStatus, MultiTaskMutex, 3
);

build_channel!(
/// Used on I2C thread, obviously.
	I2c, I2cCommand, MultiTaskMutex, 8
);
