use crate::prelude::*;

use std::ptr::{read_volatile, write_volatile};

use esp_idf_svc::hal::gpio::{Gpio9, PinDriver, Pull};

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
pub async fn reset_usb_gpio() {
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
pub async fn boot_button_pressed(input: &mut Gpio9<'_>) -> bool {
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