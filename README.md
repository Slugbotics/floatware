# Toolchain installation instructions

We are using the [esp-idf-template](https://github.com/esp-rs/esp-idf-template). These instructions are condensed from there.

1. Ensure these packages are installed via system package manager:
`rustup git wget flex bison gperf python3 python3-pip python3-venv cmake ninja-build ccache libffi-dev libssl-dev dfu-util libusb-1.0-0 riscv32-elf-gdb`
(Python 3.7+)

2. `$ pip3 install esp-idf-monitor`

3. `$ rustup default stable`

4. `$ cargo install ldproxy espflash cargo-espflash espup --locked`

5. `$ ~/.cargo/bin/espup install -t esp32,esp32c3 -f export-esp.sh`

6. `$ . export-esp.sh` (MUST BE RUN EVERYTIME A NEW TERMINAL IS IN USE)

TODO: The CI script manages to build the binary without doing all these steps. Figure out how.

# Building

`cargo build`

# Flashing

`~/.cargo/bin/espflash flash target/riscv32imc-esp-espidf/debug/floatware`, optionally add `--monitor`

(command untested)

# Improved monitoring with backtrace decoding
IDK if `--monitor` on the above command does this or not, I'll have to test that (I don't think it does)

TODO: change `.cargo/config.toml` to use the improved monitoring

Also I don't know if the argument to decode panic backtrace is correct

`python3 -m esp_idf_monitor [-p <THE DEVICE PORT>] --toolchain-prefix riscv32-esp-elf- --target esp32c3 --decode-panic backtrace target/riscv32imc-esp-espidf/debug/floatware`

(command untested)
