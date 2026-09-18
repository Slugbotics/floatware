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
	debugging::{
		ProfilingData,
		enable_profiling
	},
	profile,
};

use std::{
	ptr::{read_volatile, write_volatile},
	sync::OnceLock
};

use esp_idf_svc::{
	eventloop::EspSystemEventLoop,
	hal::{
		gpio::{Gpio9, PinDriver, Pull},
		peripherals::Peripherals
	},
	nvs::EspDefaultNvsPartition,
	timer::EspTaskTimerService,
	sys::{xTaskGetCurrentTaskHandle, TaskHandle_t}
};

use ws2812_esp32_rmt_driver::{driver::color::LedPixelColorGrbw32, LedPixelEsp32Rmt, RGB8};

////////////////////////////////////////////////////////////////////////////////

/// Macro that gets a receiver from a watch. There is a cap to the number of
/// receivers per watch that we configure in [crate::signals], so create a better
/// error message if we forget to set it high enough.
macro_rules! wrecv {
    ($channel:expr) => {
		($channel).receiver()
			.ok_or(AnyhowError::msg(concat!("Must increase max ", stringify!($channel), " receiver count")))?
	};
}

macro_rules! take {
    ($obj:ident) => {
		$obj::take().map_err(damn!(concat!("Unable to acquire ", stringify!($obj))))?
	};
}

static FLOAT_THREAD_HANDLE: OnceLock<usize> = OnceLock::new();

/// Using this in functions outside the float thread is probably not safe
#[inline] pub fn get_float_thread_handle() -> TaskHandle_t { *FLOAT_THREAD_HANDLE.get().unwrap() as _ }

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
	FLOAT_THREAD_HANDLE.set(unsafe { xTaskGetCurrentTaskHandle() as usize}).unwrap(/* unreachable */);

	////////////////////////////////////////
	// Get handles to hardware resources

	let mut peripherals = take!(Peripherals);
	let should_reset = boot_button_pressed(&mut peripherals.pins.gpio9).await;

	let sys_loop = take!(EspSystemEventLoop);
	let timer_service = EspTaskTimerService::new()?;
	let nvs = take!(EspDefaultNvsPartition);

	////////////////////////////////////////
	// Create non-static communication channels

	let status_channel = SystemStatusUnsplitWatch::new();
	let led_state_channel = LedStateSignal::new();
	let stepper_state_channel = StepperStateSignal::new();
	let button_led_signal = LedColorSignal::new();

	////////////////////////////////////////
	// Initialize external free-running systems

	let sd_card_driver = setup_sd_card(
		peripherals.spi2, // SPI1 is very limited
		peripherals.pins.gpio5, // SCLK
		peripherals.pins.gpio4, // MOSI
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

	if config.profiling { enable_profiling() }

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

	let _http_server = http::initialize_http_server(
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
		profile!(depth_target_update_task(
			wrecv!(CHARTER_STATE_CHANNEL),
			CHARTER_STATE_CHANNEL.sender(),
			&config.charter,
		)),
		profile!(depth_control_task(
			wrecv!(CHARTER_STATE_CHANNEL),
			wrecv!(status_channel),
			&stepper_state_channel, // writer
		)),
		// profile!(stepper_control_task(
		// 	config.use_uart.then_some(peripherals.uart1),
		// 	peripherals.pins.gpio18.into(), // RX + TX
		// 	peripherals.pins.gpio20, // dir
		// 	peripherals.pins.gpio21, // step
		// 	&stepper_state_channel, // reader
		// 	wrecv!(RELEASE_UART_CHANNEL),
		// )),
		profile!(leak_detection_task(
			peripherals.pins.gpio3,
			&SHUTDOWN_CHANNEL, // writer
		)),
		// ========== I/O ==========
		profile!(boot_button_pressed_task(
			peripherals.pins.gpio9,
			&button_led_signal, // writer
			RELEASE_UART_CHANNEL.sender(), // writer
		)),
		profile!(led_selection_task(
			&led_state_channel, // writer
			wrecv!(status_channel),
			&button_led_signal, // reader
			wrecv!(RELEASE_UART_CHANNEL),
		)),
		profile!(led_cycle_task(
			led_driver,
			&led_state_channel, // reader
		)),
		profile!(sd_logging_task(
			wrecv!(status_channel),
			&sd_fs_handle,
		)),
		// ========== Internal Housekeeping ==========
		profile!(status_publishing_task(
			status_channel.sender(),
			I2C_CHANNEL.sender(),
			wrecv!(CHARTER_STATE_CHANNEL),
		)),
		profile!(shutdown_task(
			&SHUTDOWN_CHANNEL, // reader
			CHARTER_STATE_CHANNEL.sender()
		)),
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

/// Reset the USB data pins (GPIO 18 & 19) so that they can be used for USB
/// communication. **Requires that those pins be disabled** (i.e. no active GPIO
/// driver).
///
/// Based on the provided esp-idf C code:
/// ```c
/// #include "soc/soc_caps.h"
/// #include "soc/usb_serial_jtag_reg.h"
/// #include "hal/usb_serial_jtag_ll.h"
///
/// SET_PERI_REG_MASK(USB_SERIAL_JTAG_CONF0_REG, USB_SERIAL_JTAG_PAD_PULL_OVERRIDE);
/// CLEAR_PERI_REG_MASK(USB_SERIAL_JTAG_CONF0_REG, USB_SERIAL_JTAG_DP_PULLUP);
/// SET_PERI_REG_MASK(USB_SERIAL_JTAG_CONF0_REG, USB_SERIAL_JTAG_DP_PULLDOWN);
///
/// vTaskDelay(pdMS_TO_TICKS(10));
///
/// #if USB_SERIAL_JTAG_LL_EXT_PHY_SUPPORTED
/// 	usb_serial_jtag_ll_phy_enable_external(false);  // Use internal PHY
/// 	usb_serial_jtag_ll_phy_enable_pad(true);        // Enable USB PHY pads
/// #else // USB_SERIAL_JTAG_LL_EXT_PHY_SUPPORTED
/// 	usb_serial_jtag_ll_phy_set_defaults();          // External PHY not supported. Set default values.
/// #endif // USB_WRAP_LL_EXT_PHY_SUPPORTED
///
/// CLEAR_PERI_REG_MASK(USB_SERIAL_JTAG_CONF0_REG, USB_SERIAL_JTAG_DP_PULLDOWN);
/// SET_PERI_REG_MASK(USB_SERIAL_JTAG_CONF0_REG, USB_SERIAL_JTAG_DP_PULLUP);
/// CLEAR_PERI_REG_MASK(USB_SERIAL_JTAG_CONF0_REG, USB_SERIAL_JTAG_PAD_PULL_OVERRIDE);
/// ```
/// Source: https://docs.espressif.com/projects/esp-iot-solution/en/latest/usb/usb_overview/usb_serial_jtag.html#using-usb-serial-jtag-pins-as-normal-gpio
///
/// esp32c3 does not have `USB_SERIAL_JTAG_LL_EXT_PHY_SUPPORTED`, so we do:
/// ```c
/// FORCE_INLINE_ATTR void usb_serial_jtag_ll_phy_set_defaults(void) {
///     USB_SERIAL_JTAG.conf0.phy_sel = 0;
///     USB_SERIAL_JTAG.conf0.usb_pad_enable = 1;
/// }
/// ```
/// Source: https://github.com/espressif/esp-idf/blob/v5.5.3/components/hal/esp32c3/include/hal/usb_serial_jtag_ll.h#L194
///
/// The data structure we need to modify is:
/// ```c
/// union {
/// 	struct {
/// 		uint32_t phy_sel             : 1; // 0
/// 		uint32_t exchg_pins_override : 1; // 1
/// 		uint32_t exchg_pins          : 1; // 2
/// 		uint32_t vrefh               : 2; // 3
/// 		uint32_t vrefl               : 2; // 5
/// 		uint32_t vref_override       : 1; // 7
/// 		uint32_t pad_pull_override   : 1; // 8
/// 		uint32_t dp_pullup           : 1; // 9
/// 		uint32_t dp_pulldown         : 1; // 10
/// 		uint32_t dm_pullup           : 1; // 11
/// 		uint32_t dm_pulldown         : 1; // 12
/// 		uint32_t pullup_value        : 1; // 13
/// 		uint32_t usb_pad_enable      : 1; // 14
/// 		uint32_t reserved15          :17;
/// 	};
/// 	uint32_t val;
/// } /*usb_serial_jtag_dev_s.*/conf0;
/// ```
/// Source: https://github.com/espressif/esp-idf/blob/v5.5.3/components/soc/esp32c3/register/soc/usb_serial_jtag_struct.h#L104
///
/// So we set `pad_pull_override`, clear `dp_pullup`, set `dp_pulldown`, wait 10ms,
/// clear `phy_sel`, set `usb_pad_enable`, clear `dp_pulldown`, set `dp_pullup`, clear
/// `pad_pull_override`.
async fn reset_usb_gpio() {
	/// https://github.com/espressif/esp-idf/blob/v5.5.3/components/soc/esp32c3/register/soc/reg_base.h#L46
	const DR_REG_USB_SERIAL_JTAG_BASE: usize = 0x60043000;
	/// https://github.com/espressif/esp-idf/blob/v5.5.3/components/soc/esp32c3/register/soc/usb_serial_jtag_reg.h#L39
	const USB_SERIAL_JTAG_CONF0_REG: *mut u32 = (DR_REG_USB_SERIAL_JTAG_BASE + 0x18) as _;

	const PHY_SEL           : usize = 0;
	const PAD_PULL_OVERRIDE : usize = 8;
	const DP_PULLUP         : usize = 9;
	const DP_PULLDOWN       : usize = 10;
	const USB_PAD_ENABLE    : usize = 14;

	fn set<const BIT: usize>() {
		unsafe {
			let current = read_volatile(USB_SERIAL_JTAG_CONF0_REG);
			write_volatile(USB_SERIAL_JTAG_CONF0_REG, current | (1 << BIT));
		}
	}

	fn clear<const BIT: usize>() {
		unsafe {
			let current = read_volatile(USB_SERIAL_JTAG_CONF0_REG);
			write_volatile(USB_SERIAL_JTAG_CONF0_REG, current & !(1 << BIT));
		}
	}

	set::<PAD_PULL_OVERRIDE>();
	clear::<DP_PULLUP>();
	set::<DP_PULLDOWN>();

	sleep_ms!(10);

	clear::<PHY_SEL>();
	set::<USB_PAD_ENABLE>();

	clear::<DP_PULLDOWN>();
	set::<DP_PULLUP>();
	clear::<PAD_PULL_OVERRIDE>();
}

/// Creates a short-lived input driver to read the state of GPIO9's button.
async fn boot_button_pressed(input: &mut Gpio9<'_>) -> bool {
	match PinDriver::input(
		// We need (temporary) ownership of the pin, so we reborrow it, knowing that the
		// driver that owns this reference will be dropped at the end of this block.
		unsafe { input.reborrow() },
		Pull::Up
	) {
		Ok(mut driver) => {
			// Button is active-low
			if driver.is_low() {
				// wait for release before continuing, so that it does not accidentally
				// trigger the button task
				if let Err(error) = driver.wait_for_high().await {
					warn!("Error waiting for boot button to be unpressed: {error:?}");
				}
				true
			} else {
				false
			}
		}
		Err(error) => {
			warn!("Unable to create input driver to check boot button: {error:#?}");
			false
		}
	}
}
