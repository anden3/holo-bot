#!/bin/bash

set -o errexit
set -o nounset
set -o pipefail
set -o xtrace

readonly TARGET_HOST=pi@rpi
readonly TARGET_PATH=/home/pi/Documents/Rust/holo-bot
readonly TARGET_ARCH=armv7-unknown-linux-gnueabihf
readonly SOURCE_PATH=./target/${TARGET_ARCH}/release/holo-bot

declare -a dependencies=(
	holo_bot.db
	settings.json
)

cargo build --release --target=${TARGET_ARCH}
rsync $SOURCE_PATH "${dependencies[@]}" ${TARGET_HOST}:${TARGET_PATH}
# ssh -t ${TARGET_HOST} ${TARGET_PATH}
