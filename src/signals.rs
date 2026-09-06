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
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use futures::channel::oneshot::Sender;
use smart_leds_trait::RGB8;
////////////////////////////////////////////////////////////////////////////////

// TODO: When finished, convert all the Watches that only have one reader into Signals
// TODO: Make a macro to generate watches

/// Signal that tells the float-thread to exit. Written from the HTTP `/shutdown`.
pub(crate) type ShutdownSignal = Signal<EspRawMutex, ShutdownRequest>;
/// Used to send commands to the stepper motor. Only used on float thread.
pub(crate) type StepperStateSignal = Signal<NoopRawMutex, StepperState>;
/// Used to send LED color information between tasks. Only used on float thread.
pub(crate) type LedColorSignal = Signal<NoopRawMutex, RGB8>;
/// Used to set the current LED cycle. Only used on float thread.
pub(crate) type LedStateSignal = Signal<NoopRawMutex, LEDState>;

// Written by HTTP `/start_dive`
const CHARTER_STATE_RECEIVERS: usize = 3;
pub(crate) type CharterStateUnsplitWatch = Watch<EspRawMutex, CharterState, CHARTER_STATE_RECEIVERS>;
pub(crate) type CharterStateReceiver<'a> = WatchReceiver<'a, EspRawMutex, CharterState, CHARTER_STATE_RECEIVERS>;
pub(crate) type CharterStateSender<'a> = WatchSender<'a, EspRawMutex, CharterState, CHARTER_STATE_RECEIVERS>;

// Written from HTTP thread
const UART_RELEASE_RECEIVERS: usize = 2;
pub(crate) type UartReleaseUnsplitWatch = Watch<EspRawMutex, (), UART_RELEASE_RECEIVERS>;
pub(crate) type UartReleaseReceiver<'a> = WatchReceiver<'a, EspRawMutex, (), UART_RELEASE_RECEIVERS>;
pub(crate) type UartReleaseSender<'a> = WatchSender<'a, EspRawMutex, (), UART_RELEASE_RECEIVERS>;

// Not used outside float thread
const POWER_MEASUREMENT_REQUEST_BUFFER_SIZE: usize = 8;
pub(crate) type PowerMeasurementRequestUnsplitChannel = Channel<NoopRawMutex, Sender<PowerMeasurement>, POWER_MEASUREMENT_REQUEST_BUFFER_SIZE>;
pub(crate) type PowerMeasurementRequestReceiver<'a> = ChannelReceiver<'a, NoopRawMutex, Sender<PowerMeasurement>, POWER_MEASUREMENT_REQUEST_BUFFER_SIZE>;
pub(crate) type PowerMeasurementRequestSender<'a> = ChannelSender<'a, NoopRawMutex, Sender<PowerMeasurement>, POWER_MEASUREMENT_REQUEST_BUFFER_SIZE>;

// Will be used in http thread
const STATUS_RECEIVERS: usize = 2;
pub(crate) type StatusUnsplitWatch = Watch<EspRawMutex, SystemStatus, STATUS_RECEIVERS>;
pub(crate) type StatusReceiver<'a> = WatchReceiver<'a, EspRawMutex, SystemStatus, STATUS_RECEIVERS>;
pub(crate) type StatusSender<'a> = WatchSender<'a, EspRawMutex, SystemStatus, STATUS_RECEIVERS>;

// Used on I2C thread, obviously.
const I2C_BUFFER_SIZE: usize = 8;
pub(crate) type I2cUnsplitChannel = Channel<EspRawMutex, I2cCommand, I2C_BUFFER_SIZE>;
pub(crate) type I2cSender<'a> = ChannelSender<'a, EspRawMutex, I2cCommand, I2C_BUFFER_SIZE>;
pub(crate) type I2cReceiver<'a> = ChannelReceiver<'a, EspRawMutex, I2cCommand, I2C_BUFFER_SIZE>;
