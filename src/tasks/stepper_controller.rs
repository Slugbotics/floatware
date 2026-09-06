use crate::prelude::*;

use esp_idf_svc::{
	hal::{
		gpio::{
			AnyIOPin,
			InputPin,
			OutputPin,
			PinDriver
		},
		uart::{
			config::Config as UartConfig,
			AsyncUartDriver,
			Uart
		},
	}
};

use futures::future::{select, Either};

////////////////////////////////////////////////////////////////////////////////

pub type StepperState = (/* TODO */);

/// TODO
pub async fn stepper_control_task(
	uart_controller: impl Uart,
	rx_pin: impl InputPin,
	tx_pin: impl OutputPin,
	dir_pin: impl OutputPin,
	step_pin: impl OutputPin,
	stepper_state_channel: &StepperStateSignal,
	mut release_uart_signal: UartReleaseReceiver<'_>,
) -> Void {
	// When the UART controller assumes control of the UART pins, it
	// clobbers USB serial communication, including flashing & console.
	// So, in order to make reflashing easier, we need to have a way to drop
	// the UART controller and let it release its associated pins. So when
	// we receive a release UART signal, we must return.

	let uart_driver = AsyncUartDriver::new(
		uart_controller, tx_pin, rx_pin,
		None::<AnyIOPin>, None::<AnyIOPin>, // CTS & RTS pins, which we don't use
		&UartConfig {
			..Default::default() // TODO
		}).map_err(damn!("Failed to initialize UART driver"))?;

	let dir_driver = PinDriver::output(dir_pin)
		.map_err(damn!("Failed to initialize stepper DIR pin"))?;
	let step_driver = PinDriver::output(step_pin)
		.map_err(damn!("Failed to initialize stepper STEP pin"))?;

	loop {
		// Concurrently await stepper commands or a UART release signal
		let future = select(stepper_state_channel.wait(), release_uart_signal.get()).await;

		// Pull out stepper state if we get it, otherwise we got the UART release signal so we need
		let stepper_state = if let Either::Left((s, _)) = future { s } else { return Ok(()) };

	}
}
