# Prototype

This is a prototype of wasm on embedded.

## Usage

To use this script:

```sh
./build.sh [release | debug] [--flash | -f] [--model | -m <MODEL>]
```

### Parameters

* release or debug: Specify the build mode (default: debug).
* -f or --flash: Enable flashing of the built firmware to the target device (optional).
* -m or --model: Select an embedded model to build (optional).

### FAQ

- Docker start failed with mount error?

  Remove or replace the run args `--mount type=bind,source=/run/udev,target=/run/udev,readonly` in `devcontainer.json`.

- Out of memory when build or test?

  Reduce the workers usage. Either `--jobs 1` or build single target `--package server --lib` will works.
