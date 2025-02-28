#!/bin/bash

cd protocol && cargo clean && cargo check && cd ..
cd server && cargo clean && cargo +stable check && cd ..
cd program && cargo clean && cargo +esp check --target xtensa-esp32s3-espidf && cd ..