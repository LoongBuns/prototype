#!/bin/bash
set -euo pipefail

BUILD_MODE="debug"
FLASH=false
MODEL="std"

declare -A DEVICE_PROFILES=(
    ["esp32"]="esp:esp:xtensa-esp32-espidf:web-flash"
    ["esp32c2"]="esp:nightly:riscv32imc-esp-espidf:web-flash"
    ["esp32c3"]="esp:nightly:riscv32imc-esp-espidf:web-flash"
    ["esp32c6"]="esp:nightly:riscv32imac-esp-espidf:web-flash"
    ["esp32h2"]="esp:nightly:riscv32imac-esp-espidf:web-flash"
    ["esp32s2"]="esp:esp:xtensa-esp32s2-espidf:web-flash"
    ["esp32s3"]="esp:esp:xtensa-esp32s3-espidf:web-flash"
)

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
        "-m" | "--model")
            if [ $# -lt 2 ]; then
                echo "Error: --model requires an argument" >&2
                exit 1
            fi
            MODEL="$2"
            shift 2
            ;;
        *)
            echo "Usage: $0 [debug|release] [--flash] [--model <MODEL>]"
            echo "Supported models: std ${!DEVICE_PROFILES[*]}"
            exit 1
            ;;
    esac
done

resolve_device_config() {
    local model=$1

    if [[ -v DEVICE_PROFILES["$model"] ]]; then
        IFS=':' read -ra config <<< "${DEVICE_PROFILES[$model]}"
        echo "samples/${config[0]} +${config[1]} ${config[2]} ${config[3]}"
    elif [[ "$model" == "std" ]]; then
        echo "samples/std"
    else
        echo "Error: Unsupported model '$model'. Supported: std ${!DEVICE_PROFILES[*]}" >&2
        exit 1
    fi
}

read -r EMBEDDED_PROJECT_DIR EMBEDDED_TOOLCHAIN EMBEDDED_TARGET FLASH_TOOL <<< $(resolve_device_config "$MODEL")

build_server() {
    local args=()
    [[ "$BUILD_MODE" == "release" ]] && args+=(--release)
    
    echo "Building server in $BUILD_MODE mode..."
    cargo +stable build --package server "${args[@]}"
}

build_device() {
    local args=()

    [[ -n "$EMBEDDED_TARGET" ]] && args+=(--target "$EMBEDDED_TARGET")
    [[ "$BUILD_MODE" == "release" ]] && args+=(--release)

    echo "Building embedded for $MODEL in $BUILD_MODE mode..."
    (
        if [[ "$MODEL" == esp* ]]; then
            if ! command -v idf.py &>/dev/null; then
                echo "Loading ESP environment..."
                export MCU="$MODEL"
                source ~/export-esp.sh >/dev/null 2>&1
            fi
        fi

        set -e
        cd "$EMBEDDED_PROJECT_DIR" || { echo "Adapter directory not found"; exit 1; }
        cargo $EMBEDDED_TOOLCHAIN build "${args[@]}"
        cd ../..
    ) || exit 1
}

flash_device() {
    local flash_cmd=""

    case "$MODEL" in
        esp*)
            flash_cmd="$FLASH_TOOL --chip $MODEL $EMBEDDED_PROJECT_DIR/target/$EMBEDDED_TARGET/$BUILD_MODE/program"
            ;;
        *)
            flash_cmd="cd $EMBEDDED_PROJECT_DIR && cargo +stable run"
            ;;
    esac

    echo "Flashing with: $flash_cmd"
    cargo +stable run --package server &
    eval "$flash_cmd" &
    wait
}

# Main
build_server
build_device

if $FLASH; then
    flash_device
fi
