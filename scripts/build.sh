#!/bin/bash

export OPUS_NO_PKG=1

set -o errexit

for i in "$@"
do
case $i in
    -p=*|--profile=*)
        PROFILE="${i#*=}"
        shift # past argument=value
        ;;
    -e=*|--environment=*)
        ENVIRONMENT="${i#*=}"
        shift # past argument=value
        ;;
    -a=*|--prod_arch=*)
        PROD_ARCH="${i#*=}"
        shift # past argument=value
        ;;
    *)
        echo "Unknown option ${i#*=}!"
        exit 1    
        ;;
esac
done

case $PROFILE in
    debug)
        PROFILE=""
        ;;
    release)
        PROFILE="--release"
        ;;
esac

case $ENVIRONMENT in
    dev|development)
        RUSTFLAGS="${RUSTFLAGS} -C target-cpu=native"
        RUSTC_FORCE_INCREMENTAL=1
        cargo build $PROFILE
        ;;
    prod|production)
        RUSTFLAGS="${RUSTFLAGS} -C target-cpu=cortex-a72 -C target-feature=+neon,+crc,+a72 -C link-arg=-march=armv8-a+crc+simd"
        RUSTC_FORCE_INCREMENTAL=1
        mold -run cargo build $PROFILE --target=$PROD_ARCH
        ;;
esac