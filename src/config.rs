use crate::charter::Charter;

use serde::{Deserialize, Serialize};

use heapless::String as HeaplessString;

#[derive(Deserialize, Serialize, Debug)]
pub struct SystemConfig {
	pub wifi_ssid: HeaplessString<32>,
	pub wifi_pass: HeaplessString<64>,
	pub charter: Charter,
	pub profiling: bool,
}

impl Default for SystemConfig {
	fn default() -> Self {
		SystemConfig {
			wifi_ssid: "ESP".try_into().unwrap(), // Will never fail; less than character limit
			wifi_pass: "floatware".try_into().unwrap(), // ditto
			charter: Default::default(),
			profiling: false,
		}
	}
}
