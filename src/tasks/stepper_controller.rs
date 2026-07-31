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

////////////////////////////////////////////////////////////////////////////////

pub type StepperState = (/* TODO */);

/// TODO: Tharuka
pub async fn stepper_control_task(
	uart_controller: impl Uart,
	rx_pin: impl InputPin,
	tx_pin: impl OutputPin,
	dir_pin: impl OutputPin,
	step_pin: impl OutputPin,
	stepper_state_channel: &StepperStateSignal,
) -> Never {
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
		let state = stepper_state_channel.wait().await;
		// TODO
	}
}
