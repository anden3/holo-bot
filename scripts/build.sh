#!/bin/bash

export OPUS_NO_PKG=true
export OPUS_STATIC=true

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
        RUSTFLAGS="${RUSTFLAGS} -C target-cpu=native -C link-arg=-fuse-ld=lld"
        cargo build $PROFILE
        ;;
    prod|production)
        RUSTFLAGS="${RUSTFLAGS} -C target-cpu=cortex_a72 -C target-feature=+neon,+crc,+a72 -C link-arg=-march=armv8-a+crc+simd"
        mold -run cargo build $PROFILE --target=$PROD_ARCH
        ;;
esac