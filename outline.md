### Goals of the floatware (float firmware):
- Communicate over WiFi via HTTP server
	- Send data packets containing:
		- Timestamp
		- Time since dive start(?)
		- General system status
		- Pressure
		- IMU information?
	- Receive data packets containing:
		- Time synchronization data
		- Course charter
		- General system config flags
		- Start/stop/reboot etc.
	- There will be multiple types of data packets, I imagine in particular the outbound ones will have multiple types
	- Good reason for ESP to be the HTTP *server* (as opposed to client to a topside server): there is no specific data
	packet that needs to be pushed to the client at a precise time
	- Data packet format: probably MessagePack because the leading Rust serialization library `serde`
	[supports it](https://github.com/3Hren/msgpack-rust) and [so does Python](https://github.com/msgpack/msgpack-python)
	and it's a compact & efficient binary format
- Maintain a persistent data store on an SD card
	- Probably via FAT fs accessed through MMC drivers (ideally - if `esp-idf-sys` supports it; otherwise SPI)
- Leak sensor loop: read value of leak sensor and initiate emergency shutdown if true
	- Ideally research how to impl in hardware
- Pressure measurement: sample on regular basis, store in memory (to be read by SD synchronizer & web server)
- Status LED: indicate conditions; we have full RGB so we can do a lot, perhaps:
	- white = standby
	- dark blue = sinking
	- green = holding position
	- yellow = rising
	- perhaps pulse pink on TX/RX
	- red = error obviously
- Stepper control: takes in ballast target size instructions & translates to stepper motion
- Buoyancy loop: uses currently measured depth and target depth to determine how much to change the ballast tank levels
- Charter control - adjusts target depth based on predetermined plan

### `main` initial setup
- Stepper (GPIO, possibly communication protocol e.g. I2C, external controller init as needed)
- Pressure sensor (ditto)
- Status LED (GPIO)
- SD card (GPIO, MMC/SPI, FS)
- Onboard flash key-value store, needed for wireless, also may be a good way to store data in conjunction with SD??
- IMU (onboard I2C)
- Temperature & humidity sensor (onboard I2C) (not strictly necessary but it's eeffoc)
- WiFi (read network settings from SD card perhaps)
- HTTP server (ditto)
- Spawn loops to handle the above

### Planned modules
"Module" not referring to a Rust module but a generally somewhat self-contained piece of code that does ~one thing,
implemented as a looping and waiting esp task  
I haven't yet looked into what the system interfaces (I2C, GPIO, ADC, etc.) look like and if multiple different threads
can access them concurrently, we may need synchronizing wrappers

- WiFi may or may not require an explicit module that handles requests with some boilerplate, it's not clear yet
	- Service dependencies: WiFi (maybe)
- HTTP module that handles incoming requests
	- Shared reads: pressure log, IMU, T&H, power stats
	- Shared writes: time synchronization data, general config & state, course charter
	- Service dependencies: HTTP server (duh), possibly I2C if we instead make it get values on-demand
- Pressure measurement module that reads and saves the data from the pressure sensor
	- Shared writes: pressure log
	- Service dependencies: I2C
	- Hardware dependencies:
		- I2C connection 
- SD module that periodically writes a log to the SD card and is used for initial config
	- Shared reads: basically everything (pressure, IMU, T&H)
	- Shared writes: none continuously, but may write config read from SD on startup
	- Service dependencies: MMC or SPI
	- Hardware dependencies:
		- 4 GPIO pins (if SPI, `GPIO[4:7]`, for MMC 4 unknown pins that may be customizable)
- Stepper control that takes stepper position instructions and physically executes them
	- I will probably implement this module using message passing instead of shared memory like the above, but I haven't
	completely settled on that, and might just do shared memory for consistency
	- Shared reads: none (∵ message passing), otherwise desired stepper state
	- Shared writes: none? (status maybe?)
	- Service dependencies: GPIO (or maybe I2C depending on how fancy the controller is)
	- Hardware dependencies:
		- 2-4 GPIO pins
- Buoyancy control that takes a target depth (or pressure?), reads the current, and tells the stepper controller how to
	change to achieve that target
	- Again, not sure if this will be mutex or message passing, but I prefer the latter
	- Shared reads: pressure log
- Course charter that periodically updates the target depth
	- Shared writes: target depth (or possibly via message passing) 
- Status LEDs + possibly LCD
	- Shared reads: system status
	- Service dependencies: GPIO, I2C if LCD
	- Hardware dependencies:
		- Onboard ARGB LED: `GPIO2`
		- More GPIOs for other LEDs, I2C connection if LCD
- Leak sensor that shuts off the stepper control modules and manually directs it
	- Shared writes: stepper control settings (which may then be message-passed instead)
	- Service dependencies: GPIO
	- Hardware dependencies:
		- 1 GPIO pin (maybe we can get faster results if we use ADC?)
- Power measurement using a current sensor and resistive divider for voltage
	- Shared writes: some power log?
	- Service dependencies: GPIO, ADC
	- Hardware dependencies:
		- 2 ADC GPIO pins

I haven't decided what data structure to use for the in-memory logs, maybe `Arc<Mutex<Box<HashMap<u32, CustomStruct>>>>`
storing a custom data struct associated with a timestamp (with a GC routine to clear older logs)
