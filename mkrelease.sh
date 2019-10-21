#!/bin/bash
# This script depends on a docker image already being built
# To build it, 
# cd docker
# docker build --tag rustbuild:latest .

POSITIONAL=()
while [[ $# -gt 0 ]]
do
key="$1"

case $key in
    -v|--version)
    APP_VERSION="$2"
    shift # past argument
    shift # past value
    ;;
    *)    # unknown option
    POSITIONAL+=("$1") # save it in an array for later
    shift # past argument
    ;;
esac
done
set -- "${POSITIONAL[@]}" # restore positional parameters

if [ -z $APP_VERSION ]; then echo "APP_VERSION is not set"; exit 1; fi

# Clean everything first
cargo clean

# Compile for mac directly
cargo build --release 

#macOS
rm -rf target/macOS-zecwallet-cli-v$APP_VERSION
mkdir -p target/macOS-zecwallet-cli-v$APP_VERSION
cp target/release/zecwallet-cli target/macOS-zecwallet-cli-v$APP_VERSION/

# For Windows and Linux, build via docker
docker run --rm -v $(pwd)/:/opt/zecwallet-light-cli rustbuild:latest bash -c "cd /opt/zecwallet-light-cli && cargo build --release && SODIUM_LIB_DIR='/opt/libsodium-win64/lib/' cargo build --release --target x86_64-pc-windows-gnu"

# Now sign and zip the binaries
#macOS
gpg --batch --output target/macOS-zecwallet-cli-v$APP_VERSION/zecwallet-cli.sig --detach-sig target/macOS-zecwallet-cli-v$APP_VERSION/zecwallet-cli 
cd target
cd macOS-zecwallet-cli-v$APP_VERSION
gsha256sum zecwallet-cli > sha256sum.txt
cd ..
zip -r macOS-zecwallet-cli-v$APP_VERSION.zip macOS-zecwallet-cli-v$APP_VERSION 
cd ..


#Linux
rm -rf target/linux-zecwallet-cli-v$APP_VERSION
mkdir -p target/linux-zecwallet-cli-v$APP_VERSION
cp target/release/zecwallet-cli target/linux-zecwallet-cli-v$APP_VERSION/
gpg --batch --output target/linux-zecwallet-cli-v$APP_VERSION/zecwallet-cli.sig --detach-sig target/linux-zecwallet-cli-v$APP_VERSION/zecwallet-cli
cd target
cd linux-zecwallet-cli-v$APP_VERSION
gsha256sum zecwallet-cli > sha256sum.txt
cd ..
zip -r linux-zecwallet-cli-v$APP_VERSION.zip linux-zecwallet-cli-v$APP_VERSION 
cd ..


#Windows
rm -rf target/Windows-zecwallet-cli-v$APP_VERSION
mkdir -p target/Windows-zecwallet-cli-v$APP_VERSION
cp target/x86_64-pc-windows-gnu/release/zecwallet-cli.exe target/Windows-zecwallet-cli-v$APP_VERSION/
gpg --batch --output target/Windows-zecwallet-cli-v$APP_VERSION/zecwallet-cli.sig --detach-sig target/Windows-zecwallet-cli-v$APP_VERSION/zecwallet-cli.exe
cd target
cd Windows-zecwallet-cli-v$APP_VERSION
gsha256sum zecwallet-cli.exe > sha256sum.txt
cd ..
zip -r Windows-zecwallet-cli-v$APP_VERSION.zip Windows-zecwallet-cli-v$APP_VERSION 
cd ..


