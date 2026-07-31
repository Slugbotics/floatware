use crate::{
	prelude::*,
};

use esp_idf_svc::{
	hal::{
		adc::{
			attenuation,
			Adc,
			AdcChannel,
			AdcUnit as AdcUnitType,
			oneshot::{
				AdcChannelDriver,
				AdcDriver,
				config::{
					AdcChannelConfig,
					Calibration
				}
			},
			Resolution
		},
		gpio::ADCPin,
	}
};

////////////////////////////////////////////////////////////////

/// Currently uses the raw 12-bit ADC output
#[derive(Debug, Clone)]
pub struct PowerMeasurement {
	pub voltage: f32,
	pub current: f32,
}

impl Default for PowerMeasurement {
	fn default() -> Self { Self { voltage: f32::NAN, current: f32::NAN } }
}

const ADC_MAX_VAL: f32 = 4095.;

/// TODO measure
const NOMINAL_VOLTAGE: f32 = 12.;
/// TODO measure
const VOLTAGE_DIVIDER_RATIO: f32 = 12.;

/// TODO measure
const CURRENT_SENSOR_V_PER_A: f32 = 0.04;

////////////////////////////////////////////////////////////////

macro_rules! adc_read_check_error {
    ($channel:ident) => {
		match ($channel).read() {
			Ok(v) => f32::from(v) / ADC_MAX_VAL,
			Err(error) => {
				warn!(concat!("Failed to read ", stringify!($channel), ": {:#?}"), error);
				f32::NAN
			}
		}
	};
}

/// Wait for power measurement requests to come in, and then service them by reading
/// the ADCs. **Note that the ADC inputs must not exceed 1V.**
pub async fn power_measurement_task<
	TheAdcUnit: AdcUnitType,
	AdcVoltageChannel: AdcChannel<AdcUnit = TheAdcUnit>,
	AdcCurrentChannel: AdcChannel<AdcUnit = TheAdcUnit>,
>(
	adc_unit: impl Adc<AdcUnit = TheAdcUnit>,
	voltage_pin: impl ADCPin<AdcChannel = AdcVoltageChannel>,
	current_pin: impl ADCPin<AdcChannel = AdcCurrentChannel>,
	power_measurement_requests: PowerMeasurementRequestReceiver<'_>
) -> Never {
	let adc_driver = AdcDriver::new(adc_unit)
		.map_err(damn!("Failed to initialize ADC driver"))?;

	let config = AdcChannelConfig {
		// Maximum input voltage is ~1.1 V. So a voltage divider will be needed.
		// Input voltage tolerance can be increased with greater attenuation but
		// the greatest level only gets us to about 4.4V so we might as well use
		// an external divider and then no attenuation, which nets higher precision.
		attenuation: attenuation::NONE,
		// The only available resolution on esp32c3
		resolution: Resolution::Resolution12Bit,
		// I think that doing this just makes the output more accurate for free?
		// If results are weird, consider switching to None.
		calibration: Calibration::Curve,
	};

	let mut voltage_channel = AdcChannelDriver::new(&adc_driver, voltage_pin, &config)
		.map_err(damn!("Voltage ADC setup failure"))?;
	let mut current_channel = AdcChannelDriver::new(&adc_driver, current_pin, &config)
		.map_err(damn!("Current ADC setup failure"))?;

	loop {
		// Instead of acting on each request individually, wait for potentially multiple to arrive,
		// and send them all the same measurement.
		power_measurement_requests.ready_to_receive().await;

		let measurement = PowerMeasurement {
			voltage: adc_read_check_error!(voltage_channel) * NOMINAL_VOLTAGE * VOLTAGE_DIVIDER_RATIO,
			current: adc_read_check_error!(current_channel) / CURRENT_SENSOR_V_PER_A,
		};

		while !power_measurement_requests.is_empty() {
			if let Err(error) = match power_measurement_requests.try_receive() {
				Ok(a) => a,
				Err(_) => break,
			}.send(measurement.clone()) {
				warn!("Failed to send measurement: {error:#?}");
			}
		}
	}
}
