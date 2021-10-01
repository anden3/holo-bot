#!/bin/bash

set -o errexit
set -o nounset
set -o pipefail

readonly TARGET_HOST=pi@rpi
readonly TARGET_PATH=/home/pi/Documents/Rust/holo-bot
readonly SOURCE_PATH=./target/${TARGET_ARCH}/release/holo-bot

declare -a dependencies=(
	settings/holobot.json
	settings/talents.toml
	settings/holobot.toml
)

rsync -P $SOURCE_PATH "${dependencies[@]}" ${TARGET_HOST}:${TARGET_PATH}
