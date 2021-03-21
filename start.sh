readonly TARGET_HOST=pi@rpi
readonly TARGET_PATH=/home/pi/Documents/Rust/holo-bot
readonly TARGET_EXEC=holo-bot

ssh -t ${TARGET_HOST} "cd ${TARGET_PATH}; ./${TARGET_EXEC}"