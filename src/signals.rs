//! - [NoopRawMutex](embassy_sync::blocking_mutex::raw::NoopRawMutex): good only for
//! signals within a single FreeRTOS task (i.e. one thread)
//!
//! - [EspRawMutex]: good for signals between FreeRTOS tasks
//!
//! - [CriticalSectionRawMutex](embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex):
//! suitable for signals accessed by ISRs

use crate::{
	prelude::*,
	tasks::{
		i2c::I2cCommand,
		charter::CharterState,
		led::LEDState,
		power_measurement::PowerMeasurement,
		stepper_controller::StepperState,
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
	}
};

use futures::channel::oneshot::Sender;

////////////////////////////////////////////////////////////////////////////////

// TODO: When finished, convert all the Watches that only have one reader into Signals

/// Signal that tells the float-thread to exit. Uses [EspRawMutex] because can be
/// written from the HTTP thread.
///
/// Unit because the intent is marked by simple presence of the message. Once a
/// message is sent on the channel, the system will shut down.
pub(crate) type ShutdownSignal = Signal<EspRawMutex, ShutdownRequest>;

// const SHUTDOWN_SIGNAL_RECEIVERS: usize = 2;
// pub(crate) type ShutdownSignalUnsplitWatch = Watch<EspRawMutex, (), SHUTDOWN_SIGNAL_RECEIVERS>;
// pub(crate) type ShutdownSignalReceiver<'a> = WatchReceiver<'a, EspRawMutex, (), SHUTDOWN_SIGNAL_RECEIVERS>;
// pub(crate) type ShutdownSignalSender<'a> = WatchSender<'a, EspRawMutex, (), SHUTDOWN_SIGNAL_RECEIVERS>;


const CHARTER_STATE_RECEIVERS: usize = 3;
pub(crate) type CharterStateUnsplitWatch = Watch<EspRawMutex, CharterState, CHARTER_STATE_RECEIVERS>;
pub(crate) type CharterStateReceiver<'a> = WatchReceiver<'a, EspRawMutex, CharterState, CHARTER_STATE_RECEIVERS>;
pub(crate) type CharterStateSender<'a> = WatchSender<'a, EspRawMutex, CharterState, CHARTER_STATE_RECEIVERS>;

pub(crate) type StepperStateSignal = Signal<EspRawMutex, StepperState>;

pub(crate) type LedStateSignal = Signal<EspRawMutex, LEDState>;

// TODO: perhaps make a macro to generate these

// ==== System Config Channels ====

// Instead of having a single big config struct, we have separate channels
// (Watches, to be precise) that contain the system status, which tasks can
// individually examine. That way, updates to unrelated configuration don't
// unnecessarily wake unrelated tasks.


// const CONFIG_CHARTER_RECEIVERS: usize = 2;
// pub(crate) type ConfigCharterUnsplitWatch = Watch<EspRawMutex, Charter, CONFIG_CHARTER_RECEIVERS>;
// pub(crate) type ConfigCharterReceiver<'a> = WatchReceiver<'a, EspRawMutex, Charter, CONFIG_CHARTER_RECEIVERS>;
// pub(crate) type ConfigCharterSender<'a> = WatchSender<'a, EspRawMutex, Charter, CONFIG_CHARTER_RECEIVERS>;

// ==== System Status Channels ====

// In the same way, we don't have a big status struct and channel for it. We split.

const POWER_MEASUREMENT_REQUEST_BUFFER_SIZE: usize = 8;
pub(crate) type PowerMeasurementRequestUnsplitChannel = Channel<EspRawMutex, Sender<PowerMeasurement>, POWER_MEASUREMENT_REQUEST_BUFFER_SIZE>;
pub(crate) type PowerMeasurementRequestReceiver<'a> = ChannelReceiver<'a, EspRawMutex, Sender<PowerMeasurement>, POWER_MEASUREMENT_REQUEST_BUFFER_SIZE>;
pub(crate) type PowerMeasurementRequestSender<'a> = ChannelSender<'a, EspRawMutex, Sender<PowerMeasurement>, POWER_MEASUREMENT_REQUEST_BUFFER_SIZE>;

const STATUS_RECEIVERS: usize = 2;
pub(crate) type StatusUnsplitWatch = Watch<EspRawMutex, SystemStatus, STATUS_RECEIVERS>;
pub(crate) type StatusReceiver<'a> = WatchReceiver<'a, EspRawMutex, SystemStatus, STATUS_RECEIVERS>;
pub(crate) type StatusSender<'a> = WatchSender<'a, EspRawMutex, SystemStatus, STATUS_RECEIVERS>;

// const CURRENT_DEPTH_RECEIVERS: usize = 2;
// pub(crate) type CurrentDepthUnsplitWatch = Watch<EspRawMutex, Depth, CURRENT_DEPTH_RECEIVERS>;
// pub(crate) type CurrentDepthReceiver<'a> = WatchReceiver<'a, EspRawMutex, Depth, CURRENT_DEPTH_RECEIVERS>;
// pub(crate) type CurrentDepthSender<'a> = WatchSender<'a, EspRawMutex, Depth, CURRENT_DEPTH_RECEIVERS>;

// ==== I2C Command Channels ====

const I2C_BUFFER_SIZE: usize = 8;
/// If this signature is updated, [i2c::i2c_thread] must also be updated.
///
/// TODO: check thread safety
pub(crate) type I2cUnsplitChannel = Channel<EspRawMutex, I2cCommand, I2C_BUFFER_SIZE>;
pub(crate) type I2cSender<'a> = ChannelSender<'a, EspRawMutex, I2cCommand, I2C_BUFFER_SIZE>;
pub(crate) type I2cReceiver<'a> = ChannelReceiver<'a, EspRawMutex, I2cCommand, I2C_BUFFER_SIZE>;
