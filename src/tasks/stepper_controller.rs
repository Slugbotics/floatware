use crate::prelude::*;

use esp_idf_svc::{
	hal::{
		gpio::{
			Pin,
			AnyIOPin,
			OutputPin,
			PinDriver
		},
		uart::{
			config::Config as UartConfig,
			AsyncUartDriver,
			Uart,
			UartDriver
		}
	},
	sys::{esp, gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD, gpio_num_t, gpio_pulldown_dis, gpio_pullup_en, gpio_set_direction, gpio_set_level},
};

////////////////////////////////////////////////////////////////////////////////

/// TODO
#[derive(Debug)]
pub enum StepperState {
	WeAreTooHigh,
	WeAreTooLow,
}

macro_rules! esp_unsafe_try {
    ($invocation:expr, $($args:tt)+) => {
		esp!(
			not_unsafe! { $invocation }
		).map_err(damn!($($args)+))?
	};
}

fn create_uart_driver<'a>(
	uart: impl Uart + 'a,
	mut uart_pin: AnyIOPin<'a>,
) -> Result<AsyncUartDriver<'a, UartDriver<'a>>, AnyhowError> {
	let pin = uart_pin.pin() as gpio_num_t;

	// OD high mode is hi-Z ("disabled"), which is what we want before initializing driver
	esp_unsafe_try!(
		gpio_set_level(pin, 1),
		"Unable to set UART pin high"
	);

	esp_unsafe_try!(
		gpio_set_direction(pin, gpio_mode_t_GPIO_MODE_INPUT_OUTPUT_OD),
		"Failed to configure UART pin as OD"
	);

	esp_unsafe_try!(
		gpio_pulldown_dis(pin),
		"Failed to disable UART pulldown"
	);

	esp_unsafe_try!(
		gpio_pullup_en(pin),
		"Failed to enable UART pullup"
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
	).map_err(damn!("Failed to initialize UART driver"))
}

pub async fn stepper_control_task(
	uart: impl Uart,
	uart_pin: AnyIOPin<'_>,
	dir_pin: impl OutputPin,
	step_pin: impl OutputPin,
	stepper_state_channel: &StepperStateSignal,
) -> Void {
	//! TODO
	//! This is a single-wire UART setup, so we need to be careful to avoid contention.
	//! TODO write down the really important constraints we need to obey to avoid physical damage

	let uart_driver = create_uart_driver(uart, uart_pin)?;

	let dir_driver = PinDriver::output(dir_pin)
		.map_err(damn!("Failed to initialize stepper DIR pin"))?;
	let step_driver = PinDriver::output(step_pin)
		.map_err(damn!("Failed to initialize stepper STEP pin"))?;

	loop {
		// Concurrently await stepper commands or a UART release signal
		let stepper_state = stepper_state_channel.wait().await;

		info!("Stepper state: {:?}", stepper_state);
	}
}
