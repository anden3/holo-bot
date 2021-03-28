readonly TARGET_HOST=pi@rpi
readonly TARGET_PATH=/home/pi/Documents/Rust/holo-bot
readonly TARGET_EXEC=holo-bot
readonly SERVICE_NAME=$TARGET_EXEC.service

ssh -t ${TARGET_HOST} "systemctl is-active --quiet ${SERVICE_NAME} && sudo systemctl stop ${SERVICE_NAME}; cd ${TARGET_PATH}; RUST_BACKTRACE=1 ./${TARGET_EXEC}"