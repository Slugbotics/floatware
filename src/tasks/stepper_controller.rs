use crate::prelude::*;

use esp_idf_svc::{
	hal::gpio::Pin,
	hal::{
		gpio::{
			AnyIOPin,
			OutputPin,
			PinDriver
		},
		uart::{
			config::Config as UartConfig,
			AsyncUartDriver,
			Uart
		},
	},
	sys::{esp, gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD, gpio_num_t, gpio_pulldown_dis, gpio_pullup_en, gpio_set_direction, gpio_set_level}
};

use futures::future::{select, Either};

////////////////////////////////////////////////////////////////////////////////

/// TODO
#[derive(Debug)]
pub enum StepperState {
	WeAreTooHigh,
	WeAreTooLow,
}

macro_rules! esp_unsafe_chk {
    ($invocation:expr, $($args:tt)+) => {
		if let Err(error) = esp!(
			unsafe { $invocation }
		) {
			warn!($($args)+, error);
			return None;
		}
	};
}

pub async fn stepper_control_task(
	maybe_uart: Option<impl Uart>,
	mut uart_pin: AnyIOPin<'_>,
	dir_pin: impl OutputPin,
	step_pin: impl OutputPin,
	stepper_state_channel: &StepperStateSignal,
	mut release_uart_signal: UartReleaseReceiver<'_>,
) -> Void {
	//! TODO
	//! This is a single-wire UART setup, so we need to be careful to avoid contention.
	//! TODO write down the really important constraints we need to obey to avoid physical damage

	// When the UART controller assumes control of the UART pins, it
	// clobbers USB serial communication, including flashing & console.
	// So, in order to make reflashing easier, we need to have a way to drop
	// the UART controller and let it release its associated pins. So when
	// we receive a release UART signal, we must return.
	let uart_driver = maybe_uart.and_then(|uart| {
		let pin = uart_pin.pin() as gpio_num_t;

		// OD high mode is hi-Z ("disabled"), which is what we want before initializing driver
		esp_unsafe_chk!(
			gpio_set_level(pin, 1),
			"Unable to set UART pin high: {:?}"
		);

		esp_unsafe_chk!(
			gpio_set_direction(pin, gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD),
			"Failed to configure UART pin as OD: {:?}"
		);

		esp_unsafe_chk!(
			gpio_pulldown_dis(pin),
			"Failed to disable UART pulldown: {:?}"
		);

		esp_unsafe_chk!(
			gpio_pullup_en(pin),
			"Failed to enable UART pullup: {:?}"
		);

		let rtx_pin_clone = unsafe {
			((&mut uart_pin) as *mut AnyIOPin).as_mut_unchecked().reborrow()
		};

		AsyncUartDriver::new(
			uart, uart_pin, rtx_pin_clone, // single pin for RX & TX
			None::<AnyIOPin>, None::<AnyIOPin>, // CTS & RTS pins, which we don't use
			&UartConfig {
				..Default::default() // TODO
			}
		).map_err(|_| warn!("Failed to initialize UART driver")).ok()
	});

	let dir_driver = PinDriver::output(dir_pin)
		.map_err(damn!("Failed to initialize stepper DIR pin"))?;
	let step_driver = PinDriver::output(step_pin)
		.map_err(damn!("Failed to initialize stepper STEP pin"))?;

	loop {
		// Concurrently await stepper commands or a UART release signal
		let either = select(
			stepper_state_channel.wait(),
			release_uart_signal.get() // safe to call get() because it terminates the loop
		).await;

		// Pull out stepper state if we get it, otherwise we got the UART release signal so we need
		let stepper_state = if let Either::Left((s, _)) = either { s } else {
			info!("Dropping uart");
			if let Some(uart_controller) = uart_driver { drop(uart_controller) }

			return Ok(());
		};

		info!("Stepper state: {:?}", stepper_state);
	}
}

#[derive(Clone)]
pub enum UartRelease {
	Requested, Done
}
