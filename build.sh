#!/bin/bash

BUILD_MODE=debug
FLASH=false

EMBEDDED_MODEL=esp32s3
EMBEDDED_TARGET=xtensa-$EMBEDDED_MODEL-espidf

while [ $# -gt 0 ]; do
    case "$1" in
        "release" | "debug")
            BUILD_MODE="$1"
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

if [ "$BUILD_MODE" = "release" ]; then
    cargo +stable build --release --package server
else
    cargo +stable build --package server
fi

(
    which idf.py >/dev/null || { source ~/export-esp.sh >/dev/null 2>&1; }

    if [ "$BUILD_MODE" = "release" ]; then
        cd program && cargo +esp build --release --target $EMBEDDED_TARGET && cd ../..
    else
        cd program && cargo +esp build --target $EMBEDDED_TARGET && cd ../..
    fi
)

if $FLASH; then
    web-flash --chip $EMBEDDED_MODEL program/target/$EMBEDDED_TARGET/$BUILD_MODE/program &
    (cargo +stable run --package server) &
    wait
fi
