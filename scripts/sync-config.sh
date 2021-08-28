#!/bin/bash

set -o errexit
set -o nounset
set -o pipefail

readonly TARGET_HOST=pi@rpi
readonly TARGET_PATH=/home/pi/Documents/Rust/holo-bot

declare -a files=(
	production.json
	production.db
)

echo "Syncing from remote."
rsync -uhP --append-verify ${files[@]/#/${TARGET_HOST}:${TARGET_PATH}\/} "settings/"
echo "Syncing from host."
rsync -uhP --append-verify "${files[@]/#/settings\/}" ${TARGET_HOST}:${TARGET_PATH}