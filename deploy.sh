#!/bin/bash

set -o errexit
set -o nounset
set -o pipefail

readonly TARGET_HOST=pi@rpi
readonly TARGET_PATH=/home/pi/Documents/Rust/holo-bot
readonly TARGET_ARCH=armv7-unknown-linux-gnueabihf
readonly SOURCE_PATH=./target/${TARGET_ARCH}/debug/holo-bot

export SQLITE3_LIB_DIR="/mnt/f/Languages/Rust/sqlite3/lib"

declare -a dependencies=(
	holo_bot.db
	settings.json
)

cargo build --target=${TARGET_ARCH}
rsync -P $SOURCE_PATH "${dependencies[@]}" ${TARGET_HOST}:${TARGET_PATH}
