#!/bin/bash

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
        RUSTC_FORCE_INCREMENTAL=1
        cargo build $PROFILE
        ;;
    prod|production)
        RUSTFLAGS="-C target-cpu=armv6 link-arg=-march=armv6"
        RUSTC_FORCE_INCREMENTAL=1
        mold -run cargo build $PROFILE --target=$PROD_ARCH
        ;;
esac