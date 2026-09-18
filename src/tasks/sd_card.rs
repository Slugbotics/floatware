use crate::{
	prelude::*,
	tasks::{
		status::SystemStatus
	}
};

use std::{
	fs::{File, OpenOptions},
	io::{
		ErrorKind,
		Write
	},
	time::Instant
};
use std::ffi::OsStr;
use std::fs::{read_dir, DirEntry};
use anyhow::Error;
use esp_idf_svc::{
	fs::fatfs::{
		config::{FatFsType, FormatConfiguration},
		Fatfs
	},
	hal::{
		gpio::{
			AnyIOPin,
			InputPin,
			OutputPin,
		},
		sd::{
			config::Configuration as SdConfiguration,
			spi::SdSpiHostDriver,
			SdCardDriver
		},
		spi::{
			config::DriverConfig as SpiBusConfig,
			Dma,
			SpiAnyPins,
			SpiDriver
		},
	},
	io::vfs::MountedFatfs,
};
use itertools::Itertools;
use serde::Deserialize;
use crate::config::SystemConfig;
////////////////////////////////////////////////////////////////////////////////

macro_rules! open_file {
    ($path:literal) => { OpenOptions::new().read(true).append(true).create(true).open(sd!($path)).map_err(damn!(concat!("Failed to open ", $path)))? };
    ($path:expr) => { OpenOptions::new().read(true).append(true).create(true).open(sd!($path)).map_err(damn!("Failed to open {}", $path))? };
}

pub type FSHandle<'a> = MountedFatfs<Fatfs<SdCardDriver<SdSpiHostDriver<'a, SpiDriver<'a>>>>>;

////////////////////////////////////////////////////////////////////////////////

/// Create an SD card driver using the provided pins for SPI.
pub fn setup_sd_card<'a>(
	spi_controller: impl SpiAnyPins + 'a,
	sclk: impl OutputPin + 'a,
	mosi: impl OutputPin + 'a,
	miso: impl  InputPin + 'a,
	  cs: impl OutputPin + 'a,
) -> Result<SdCardDriver<SdSpiHostDriver<'a, SpiDriver<'a>>>, AnyhowError> {
	let spi_bus_driver = SpiDriver::new(
		spi_controller, sclk, mosi, Some(miso),
		&SpiBusConfig {
			dma: Dma::Auto(4096 /* TODO */),
			intr_flags: Default::default(),
		}
	).map_err(damn!("Failed to initialize SPI bus driver"))?;

	info!("Created SPI bus driver");

	// Spi device config (CS etc)
	let sd_spi_device_driver = SdSpiHostDriver::new(
		spi_bus_driver,
		Some(cs),
		AnyIOPin::none(/* Card Detect   */),
		AnyIOPin::none(/* Write Protect */),
		AnyIOPin::none(/* Interrupt     */),
		None // Write protection config
	).map_err(damn!("Failed to initialize underlying SD SPI device driver"))?;

	info!("Initialized SDSPI");

	let sd_card_driver = SdCardDriver::new_spi(
		sd_spi_device_driver, &SdConfiguration::default(/* todo */)
	).map_err(damn!("Failed to initialize SD card driver"))?;

	info!("Initialized SD driver");

	Ok(sd_card_driver)
}

/// Attempt to mount a FAT FS on an SD card. Returns the mounted filesystem handle,
/// which should be kept around, as dropping it leads to unmounting.
pub fn mount_sd_card<'a>(
	sd_card_driver: SdCardDriver<SdSpiHostDriver<'a, SpiDriver<'a>>>,
	should_format: bool
) -> Result<FSHandle<'a>, AnyhowError> {
	info!("Beginning SD fs mount");

	let mut fatfs = Fatfs::new_sdcard(
		0, // Drive number. This is the first & only SD card, so 0.
		sd_card_driver
	).map_err(damn!("Failed to initialize FAT FS"))?;

	if should_format {
		info!("Formatting SD card");

		let mut buf = [0u8; 4096 /* TODO */];

		let config = &FormatConfiguration {
			fs_type: FatFsType::ExFat, // so we don't worry about log file sizes
			fat_backup_copy: true, // place a backup FAT table at the end(?) of the card
			..Default::default()
		};
		let start = Instant::now();
		let result = fatfs.format(config, &mut buf);
		let elapsed = start.elapsed();

		match result {
			Ok(()) => info!("Formatted SD card in {} us", elapsed.as_micros()),
			Err(error) => {
				warn!("Failed to format SD card: {error} ({} us)", elapsed.as_micros());
				return Err(error.into());
			}
		}
	}

	// Not to be confused with esp_idf_svc::fs::fatfs::MountedFatfs
	// This is the vfs fatfs mount, which provides a higher layer of abstraction and allows
	// using Rust's std File objects.
	let fs = MountedFatfs::mount(
		fatfs,
		// Mountpoint to prepend to file paths
		concat!("/", SD_CARD_NAME!()),
		// Maximum number of file descriptors to allocate. I don't know what happens if
		// you exceed this number, nor the lifetime of an fd here (related to `File` object
		// lifetime, presumably). So TODO investigate FD count
		4
	).map_err(damn!("Failed to mount filesystem"))?;

	info!("Successfully mounted SD card, found files: {}", read_dir(sd!(""))
		.map_err(damn!("Failed to read /sd/"))?
		.filter_map(Result::ok)
		.format_with(", ", |entry, f| f(&entry.file_name().to_string_lossy())
	));

	Ok(fs)
}

////////////////////////////////////////////////////////////////////////////////

pub fn read_config_from_sd() -> Result<SystemConfig, AnyhowError> {
	match File::open(sd!("CONFIG.JSON")).map_err(damn!("Failed to read config")) {
		Ok(file) => SystemConfig::deserialize(
			&mut serde_json::Deserializer::from_reader(file)
		).map_err(damn!("Failed to deserialize config")),

		Err(error) => {
			warn!("Failed to read config file: {error}");

			let config = SystemConfig::default();

			let mut file = match File::create(sd!("CONFIG.JSON")) {
				Ok(file) => file,
				Err(error) => {
					warn!("Failed to create config file: {error}");
					return Ok(config);
				},
			};

			let content = match serde_json::to_string_pretty(&config) {
				Ok(content) => content,
				Err(error) => {
					warn!("Failed to serialize default config: {error}");
					return Ok(config);
				},
			};

			if let Err(error) = file.write_all(content.as_bytes()) {
				warn!("Failed to write default config: {error}")
			}

			Ok(config)
		}
	}

}

////////////////////////////////////////////////////////////////////////////////

/// Appends to a series of log files whenever it receives [SystemStatus] objects
/// with `create_log_entry == true`.
///
/// **Blocks the entire FreeRTOS thread on write!** If the write operations are
/// expensive, it may be worthwhile to consider creating an I/O thread like I2C.
pub async fn sd_logging_task(
	mut status_receiver: SystemStatusReceiver<'_>,
	fs_handle: &Option<FSHandle<'_>>,
) -> Void {
	// We don't actually need the fs handle in order to write to the filesystem,
	// but we do need it in order to determine whether the fs exists.
	if fs_handle.is_none() {
		return Ok(()) // No SD card inserted or other error occurred attempting to mount it
	}

	let mut log_file_counter = 0u8;
	let mut log_file = open_file!("LOG000.txt");

	loop {
		let snapshot = status_receiver.changed_and(SystemStatus::create_log_entry).await;

		loop {
			match write!(&mut log_file, "{snapshot}\n") {
				Err(error) => match error.kind() {
					ErrorKind::Interrupted => continue, // Just try again
					ErrorKind::OutOfMemory => ret_err!("Out of memory"),
					ErrorKind::TooManyOpenFiles => ret_err!("Too many open files, increase FD count"),
					ErrorKind::FileTooLarge => { // Create new file
						log_file_counter += 1;
						log_file = open_file!(format!("status_log_{log_file_counter:03}.txt"));
						continue;
					}
					etc => return Err(anyhow::anyhow!("Error writing log entry: {etc:#?}")),
				},
				_ => break, // success
			}
		}
	}
}
