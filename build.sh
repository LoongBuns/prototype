#!/bin/bash

which idf.py >/dev/null || {
    source ~/export-esp.sh >/dev/null 2>&1
}

BUILD_MODE=debug
FLASH=false

EMBEDDED_MODEL=esp32s3
EMBEDDED_TARGET=xtensa-$EMBEDDED_MODEL-espidf

while [ $# -gt 0 ]; do
    case "$1" in
        "release" | "debug")
            MODE="$1"
            shift
            ;;
        "-f" | "--flash")
            FLASH=true
            shift
            ;;
        *)
            echo "Wrong argument. Only \"debug\"/\"release\" arguments are supported"
            shift
            ;;
    esac
done

if [ "$MODE" = "release" ]; then
    cargo +esp build --release --target $EMBEDDED_TARGET --package program --config program/.cargo/config.toml
    cargo +stable build --release --package server
else
    cargo +esp build --target $EMBEDDED_TARGET --package program --config program/.cargo/config.toml
    cargo +stable build --package server
fi

if $FLASH; then
    web-flash --chip $EMBEDDED_MODEL target/$EMBEDDED_TARGET/$BUILD_MODE/program
fi
