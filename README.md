```sh
which idf.py >/dev/null || {
    source ~/export-esp.sh >/dev/null 2>&1
}

TARGET="xtensa-esp32s3-espidf"

cargo +esp build --release --target $ESP_TARGET --package program
```

```sh
cargo +stable build --release --package server
```

```sh
BUILD_EXE="target/xtensa-esp32s3-espidf/release/program"

web-flash --chip esp32s3 $BUILD_EXE
```