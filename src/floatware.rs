use crate::{
	prelude::*,
	tasks::{
		button::boot_button_pressed_task,
		charter::{
			depth_control_task,
			depth_target_update_task
		},
		i2c::initialize_i2c_thread,
		leak_detection::leak_detection_task,
		led::{led_cycle_task, led_selection_task},
		power_measurement::power_measurement_task,
		sd_card::{
			mount_sd_card,
			read_config_from_sd,
			sd_logging_task,
			setup_sd_card
		},
		shutdown::{
			shutdown_task,
			Shutdown,
		},
		status::status_publishing_task,
		stepper_controller::stepper_control_task,
		*,
	},
};

use esp_idf_svc::{
	eventloop::EspSystemEventLoop,
	hal::{
		gpio::{Gpio9, PinDriver, Pull},
		peripherals::Peripherals
	},
	http::server::EspHttpServer,
	nvs::EspDefaultNvsPartition,
	sys::{uxTaskGetStackHighWaterMark2, xTaskGetCurrentTaskHandle},
	timer::EspTaskTimerService,
};

use ws2812_esp32_rmt_driver::{driver::color::LedPixelColorGrbw32, LedPixelEsp32Rmt, RGB8};

////////////////////////////////////////////////////////////////////////////////

/// Macro that gets a receiver from a watch. There is a cap to the number of
/// receivers per watch that we configure in [crate::signals], so create a better
/// error message if we forget to set it high enough.
macro_rules! wrecv {
    ($channel:expr) => {
		($channel).receiver()
			.expect(concat!("Must increase max ", stringify!($channel), " receiver count"))
	};
}
macro_rules! take {
    ($obj:ident) => {
		$obj::take().map_err(damn!(concat!("Unable to acquire ", stringify!($obj))))?
	};
}

// /// Creates an [EspAsyncTimer][esp_idf_svc::timer::EspAsyncTimer] using a provided
// /// [EspTaskTimerService].
// macro_rules! create_timer {
//     ($service:expr) => {
// 		($service).timer_async()
// 			.map_err(damn!(concat!("Failed to create timer at ", file!(), ":", line!())))?
// 	};
// }

// These are message channels that are shared across multiple threads. They need to
// be `static` so we can be assured they will always exist for all threads to make
// use of.

/// Used to tell [float_thread] to exit
static SHUTDOWN_CHANNEL: ShutdownSignal = ShutdownSignal::new();

/// Used for sending requests to the I2C driver
static I2C_CHANNEL: I2cUnsplitChannel = I2cUnsplitChannel::new();

static CHARTER_STATE_CHANNEL: CharterStateUnsplitWatch = CharterStateUnsplitWatch::new(/* no Charter */);

static RELEASE_UART_CHANNEL: UartReleaseUnsplitWatch = UartReleaseUnsplitWatch::new();

/// Where we actually start setting up everything. This function is responsible for
/// spawning the remaining threads, initializing peripherals, and spawning tasks.
pub async fn float_thread() -> Void {
	////////////////////////////////////////
	// Get handles to hardware resources

	let mut peripherals = take!(Peripherals);
	let should_reset = boot_button_pressed(&mut peripherals.pins.gpio9);

	let sys_loop = take!(EspSystemEventLoop);
	let timer_service = EspTaskTimerService::new()?;
	let nvs = take!(EspDefaultNvsPartition);

	////////////////////////////////////////
	// Create non-static communication channels

	let power_measurement_request_channel = PowerMeasurementRequestUnsplitChannel::new();
	let status_channel = StatusUnsplitWatch::new();
	let led_state_channel = LedStateSignal::new();
	let stepper_state_channel = StepperStateSignal::new();
	let button_led_signal = LedColorSignal::new();

	////////////////////////////////////////
	// Initialize external free-running systems

	let sd_card_driver = setup_sd_card(
		peripherals.spi2, // SPI1 is very limited
		peripherals.pins.gpio4, // SCLK
		peripherals.pins.gpio5, // MOSI
		peripherals.pins.gpio6, // MISO
		peripherals.pins.gpio7, // CS
	).map(Some).unwrap_or_else(|error| {
		warn!("Failed to initialize SD card: {error:#?}");
		None
	});

	// We need to keep the filesystem handle around, because once it goes out of scope and gets
	// dropped, the filesystem will be unmounted.
	let (sd_fs_handle, config) = if let Some(driver) = sd_card_driver {
		match mount_sd_card(driver, should_reset) {
			Ok(_handle) => (
				Some(_handle),
				read_config_from_sd().unwrap_or_else(|error| {
					warn!("Failed to read config: {error:#?}");
					Default::default()
				})
			),
			Err(error) => {
				warn!("Failed to mount SD card: {error:#?}");
				Default::default()
			}
		}
	} else { Default::default() };

	info!("Using {config:#?}");

	let _wifi = wifi::initialize_wifi(
		peripherals.modem,
		&sys_loop,
		&timer_service,
		&nvs,
		config.wifi_ssid,
		config.wifi_pass
	).await.map_err(damn!("Failed to initialize wifi"))?;

	let _i2c_handle = initialize_i2c_thread(
		peripherals.i2c0,
		peripherals.pins.gpio10, // SDA
		peripherals.pins.gpio8,  // SCL
		I2C_CHANNEL.receiver()
	).map_err(damn!("Failed to initialize I2C thread"))?;

	let _http_server: EspHttpServer = http::initialize_http_server(
		&SHUTDOWN_CHANNEL,
		I2C_CHANNEL.sender(),
		CHARTER_STATE_CHANNEL.sender(),
		RELEASE_UART_CHANNEL.sender(),
	).map_err(damn!("Failed to initialize HTTP server"))?;

	let led_driver =
		LedPixelEsp32Rmt::<RGB8, LedPixelColorGrbw32>::new(
			peripherals.rmt.channel0, peripherals.pins.gpio2
		).map_err(damn!("Failed to create led driver"))?;

	info!("Initialization complete. Starting event loop.");

	////////////////////////////////////////
	// Create float-thread tasks and concurrently join them

	// Any task returning an error leads to an immediate shutdown
	let Err(error) = futures::try_join!(
		// ========== Peripheral Management ==========
		depth_target_update_task(
			wrecv!(CHARTER_STATE_CHANNEL),
			CHARTER_STATE_CHANNEL.sender(),
			&config.charter,
		),
		depth_control_task(
			wrecv!(CHARTER_STATE_CHANNEL),
			wrecv!(status_channel),
			&stepper_state_channel, // writer
		),
		stepper_control_task(
			peripherals.uart0,
			peripherals.pins.gpio20, // RX
			peripherals.pins.gpio21, // TX
			peripherals.pins.gpio18, // dir
			peripherals.pins.gpio19, // step
			&stepper_state_channel, // reader
			wrecv!(RELEASE_UART_CHANNEL),
		),
		leak_detection_task(
			peripherals.pins.gpio3,
			&SHUTDOWN_CHANNEL, // writer
		),
		power_measurement_task(
			peripherals.adc1,
			peripherals.pins.gpio0, // voltage ADC
			peripherals.pins.gpio1, // current ADC
			power_measurement_request_channel.receiver(),
		),
		// ========== I/O ==========
		boot_button_pressed_task(
			peripherals.pins.gpio9,
			&button_led_signal, // writer
			RELEASE_UART_CHANNEL.sender(), // writer
		),
		led_selection_task(
			&led_state_channel, // writer
			wrecv!(status_channel),
			&button_led_signal, // reader
			wrecv!(RELEASE_UART_CHANNEL),
		),
		led_cycle_task(
			led_driver,
			&led_state_channel, // reader
		),
		sd_logging_task(
			wrecv!(status_channel),
			&sd_fs_handle,
		),
		// ========== Internal Housekeeping ==========
		status_publishing_task(
			power_measurement_request_channel.sender(),
			status_channel.sender(),
			I2C_CHANNEL.sender(),
			wrecv!(CHARTER_STATE_CHANNEL),
		),
		shutdown_task(
			&SHUTDOWN_CHANNEL, // reader
			CHARTER_STATE_CHANNEL.sender()
		),
	);

	// The shutdown task has a unique error that it returns to indicate a normal shutdown
	if let Some(Shutdown(request)) = error.downcast_ref() {
		info!("Shutdown due to {request:?}");
		Ok(())
	} else {
		Err(error)
	}
}

////////////////////////////////////////////////////////////////////////////////

/// Return the minimum recorded amount of stack space (in bytes) remaining for the
/// calling thread. It should go without saying, but don't call this from an
/// interrupt (I don't know what will happen in that case, but probably nothing
/// good)
fn high_water_mark() -> u32 { unsafe { uxTaskGetStackHighWaterMark2(xTaskGetCurrentTaskHandle()) } }

/// Creates a short-lived input driver to read the state of GPIO9's button.
fn boot_button_pressed(input: &mut Gpio9) -> bool {
	match PinDriver::input(
		// We need (temporary) ownership of the pin, so we reborrow it, knowing that the
		// driver that owns this reference will be dropped at the end of this block.
		unsafe { input.reborrow() },
		Pull::Up
	) {
		Ok(driver) => driver.is_low(), // Button is active-low
		Err(error) => {
			warn!("Unable to create input driver to check boot button: {error:#?}");
			false
		}
	}
}
