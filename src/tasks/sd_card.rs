use crate::{
	prelude::*,
	tasks::{
		charter::Charter,
		status::SystemStatus
	}
};

use std::{
	fs::{File, OpenOptions},
	io::ErrorKind,
	io::Write
};

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

use heapless::String as HeaplessString;

use serde::Deserialize;
use crate::config::SystemConfig;
////////////////////////////////////////////////////////////////////////////////

macro_rules! open_file {
    ($path:expr) => { OpenOptions::new().read(true).append(true).create(true).open(sd!($path)).map_err(damn!("Failed to open {}", $path))? };
    ($path:literal) => { OpenOptions::new().read(true).append(true).create(true).open(sd!($path)).map_err(damn!(concat!("Failed to open ", $path)))? };
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

	// Spi device config (CS etc)
	let sd_spi_device_driver = SdSpiHostDriver::new(
		spi_bus_driver,
		Some(cs),
		AnyIOPin::none(/* Card Detect   */),
		AnyIOPin::none(/* Write Protect */),
		AnyIOPin::none(/* Interrupt     */),
		None // Write protection config
	).map_err(damn!("Failed to initialize underlying SD SPI device driver"))?;

	let sd_card_driver = SdCardDriver::new_spi(
		sd_spi_device_driver, &SdConfiguration::default(/* todo */)
	).map_err(damn!("Failed to initialize SD card driver"))?;

	Ok(sd_card_driver)
}

/// Attempt to mount a FAT FS on an SD card. Returns the mounted filesystem handle,
/// which should be kept around, as dropping it leads to unmounting.
pub fn mount_sd_card<'a>(
	sd_card_driver: SdCardDriver<SdSpiHostDriver<'a, SpiDriver<'a>>>,
	should_format: bool
) -> Result<FSHandle<'a>, AnyhowError> {
	let mut fatfs = Fatfs::new_sdcard(
		0, // Drive number. This is the first & only SD card, so 0.
		sd_card_driver
	).map_err(damn!("Failed to initialize FAT FS"))?;

	if should_format {
		info!("Formatting SD card");

		let mut buf = [0u8; 4096 /* TODO */];

		fatfs.format(&FormatConfiguration {
			fs_type: FatFsType::ExFat, // so we don't worry about log file sizes
			fat_backup_copy: true, // place a backup FAT table at the end(?) of the card
			..Default::default()
		}, &mut buf).map_err(damn!("Failed to format SD card"))?;
	}

	// Not to be confused with esp_idf_svc::fs::fatfs::MountedFatfs
	// This is the vfs fatfs mount, which provides a higher layer of abstraction and allows
	// using Rust's std File objects.
	MountedFatfs::mount(
		fatfs,
		// Mountpoint to prepend to file paths
		concat!("/", SD_CARD_NAME!()),
		// Maximum number of file descriptors to allocate. I don't know what happens if
		// you exceed this number, nor the lifetime of an fd here (related to `File` object
		// lifetime, presumably). So TODO investigate FD count
		4
	).map_err(damn!("Failed to mount filesystem"))
}

////////////////////////////////////////////////////////////////////////////////

pub fn read_config_from_sd() -> Result<SystemConfig, AnyhowError> {
	SystemConfig::deserialize(
		&mut serde_json::Deserializer::from_reader(
			File::open(sd!("config.json")).map_err(damn!("Failed to read config"))?
		)
	).map_err(damn!("Failed to deserialize config"))
}

////////////////////////////////////////////////////////////////////////////////

/// Appends to a series of log files whenever it receives [SystemStatus] objects
/// with `create_log_entry == true`.
///
/// **Blocks entire FreeRTOS thread on write!** If the write operations are
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
	let mut log_file = open_file!("status_log_000.txt");

	loop {
		let snapshot = status_receiver.get_and(SystemStatus::create_log_entry).await;

		info!("LOGGING: {snapshot}");

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
