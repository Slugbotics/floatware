use embassy_sync::{
	signal::Signal,
	blocking_mutex::raw::CriticalSectionRawMutex
};

use esp_idf_svc::hal::task::embassy_sync::EspRawMutex;

use smart_leds_trait::RGB8;

pub(crate) type LedSignal = Signal<EspRawMutex, RGB8>;
pub(crate) type ShutdownSignal = Signal<CriticalSectionRawMutex, ()>;
