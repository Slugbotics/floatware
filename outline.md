### Goals of the floatware (float firmware):
-[ ] Communicate over WiFi via HTTP server
	- Send data packets containing:
		- Timestamp
		- Dive status
		- General system status
		- Pressure
		- IMU information?
	-[ ] Receive data packets containing:
		- Time synchronization data
		- Course charter
		- General system config flags
		- Start/stop/reboot etc.
	- There will be multiple types of data packets, I imagine in particular the outbound ones will have multiple types
	- Good reason for ESP to be the HTTP *server* (as opposed to client to a topside server): there is no specific data
	packet that needs to be pushed to the client at a precise time
	-[ ] Data packet format: probably MessagePack because the leading Rust serialization library `serde`
	[supports it](https://github.com/3Hren/msgpack-rust) and [so does Python](https://github.com/msgpack/msgpack-python)
	and it's a compact & efficient binary format
-[ ] Maintain a persistent data store on an SD card
	- Via FatFs accessed through SPI driver
-[x] Leak sensor loop: read value of leak sensor and initiate emergency shutdown if true
-[x] Pressure measurement: sample on regular basis, store in memory (to be read by SD synchronizer & web server)
-[ ] Status LED: indicate conditions; we have full RGB so we can do a lot, perhaps:
	- white = standby
	- dark blue = sinking
	- green = holding position
	- yellow = rising
	- perhaps pulse pink on TX/RX
	- red = error obviously
-[ ] Stepper control: takes in ballast target size instructions & translates to stepper motion
-[ ] Buoyancy loop: uses currently measured depth and target depth to determine how much to change the ballast tank levels
-[x] Charter control - adjusts target depth based on predetermined plan

### `main` initial setup
- Stepper (GPIO, possibly communication protocol e.g. I2C, external controller init as needed)
- Pressure sensor (ditto)
- Status LED (GPIO)
- SD card (GPIO, SPI, FS)
- Onboard flash key-value store, needed for wireless, also may be a good way to store data in conjunction with SD??
- IMU (onboard I2C)
- Temperature & humidity sensor (onboard I2C) (not strictly necessary but it's eeffoc)
- WiFi (read network settings from SD card perhaps)
- HTTP server (ditto)
- Spawn loops to handle the above

### Planned tasks
"Task" not referring to a FreeRTOS task but a generally somewhat self-contained piece of code that does ~one thing,
implemented as a looping and waiting async function

-[ ] I2C task
	-[ ] Depth sensor
    -[x] T&H lol
    -[ ] IMU?
-[ ] HTTP task that handles incoming requests
	- Shared reads: pressure log, IMU, T&H, power stats
	- Shared writes: time synchronization data, general config & state
	- Service dependencies: I2C
-[ ] Logkeeping task that intermittently reads data (chiefly from the pressure sensor) and creates log entries
	- Shared writes: system log
    - Service dependencies: I2C (pressure sensor)
-[ ] SD synchronization task that periodically writes a log to the SD card (and is used for initial config?)
	- Shared reads: basically everything (pressure, IMU, T&H)
	- Shared writes: none continuously, but may write config read from SD on startup
	- Service dependencies: SPI
	- Hardware dependencies:
		- 4 GPIO pins
-[ ] Stepper control that takes stepper position instructions and physically executes them
	- Shared reads: none (∵ message passing), otherwise desired stepper state
	- Shared writes: none? (status maybe?)
	- Service dependencies: GPIO (or maybe I2C depending on how fancy the controller is)
	- Hardware dependencies:
		- 2-4 GPIO pins
-[ ] Buoyancy control that takes a target depth (or pressure?), reads the current, and tells the stepper controller how to
	change to achieve that target
	- Again, not sure if this will be mutex or message passing, but I prefer the latter
	- Shared reads: pressure log
-[x] Course charter that periodically updates the target depth
	- Shared writes: target depth
-[ ] Status LEDs + possibly LCD
	- Service dependencies: GPIO
	- Hardware dependencies:
		- Onboard ARGB LED: `GPIO2`
-[ ] Leak sensor that commands the stepper to go fully open
	- Service dependencies: GPIO
	- Hardware dependencies:
		- 1 GPIO pin
-[x] Power measurement using a current sensor and resistive divider for voltage
	- Shared writes: some power log?
	- Service dependencies: GPIO, ADC
	- Hardware dependencies:
		- 2 ADC GPIO pins

### Data structures & state
- System keeps track of the current state with a struct, that includes:
	- Timestamp
	- Pressure
	- Abridged status, focusing on navigation (e.g. approximate current depth, state of charter)
	- System voltage and current (if we even have a current sensor)
	- more general system report...
- There is one "current" instance of that struct. Then, separately, there is a logging task that keeps track of a
	history of states, and intermittently writes the values to SD.

#### Concrete things I need to do
-Decide how to set LED control - perhaps the LED task has its arms in several other signals and tasks and decides
what to do based on their stati

#### Things other people will do
- Depth sensor reading code (I forgot who)
- Stepper motor driver (Tharuka)
- Buoyancy control loop (Andy)

Shutdown -> wait for stepper to fully set -> exit loop

