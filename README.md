# Prototype

This is a prototype of wasm on esp.

## Usage

To use this script:

```sh
./build.sh [--flash | -f] [release | debug]
```

### Parameters

* release or debug: Specify the build mode (default: debug).
* -f or --flash: Enable flashing of the built firmware to the ESP32-S3 device (optional).

### FAQ

- Docker start failed with mount error on Windows?

  Remove the run args `--mount type=bind,source=/run/udev,target=/run/udev,readonly` in `devcontainer.json`.
