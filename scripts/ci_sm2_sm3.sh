#!/bin/bash
set -e


SOURCE_DIR=$(readlink -f $(dirname $(readlink -f $0))/..)
cd  ${SOURCE_DIR}
source ~/.cargo/env

sed -i 's/\["secp256k1"\]/\["sm2"\]/g' share_libs/crypto/Cargo.toml
sed -i 's/\["sha3hash"\]/\["sm3hash"\]/g' share_libs/util/Cargo.toml

echo "################################################################################"
echo "1) setup"
git status
git rev-parse HEAD
scripts/config_rabbitmq.sh
# For native machine, skip this step.
# scripts/install_develop.sh

echo "################################################################################"
echo "2) format"
time make fmt

echo "################################################################################"
echo "3) build"
time make debug

echo "################################################################################"
echo "4) unit test"
time make test

echo "################################################################################"
echo "5) integrate test"
echo "5.1) basic test(contract create/call, node start/stop)"
time ./tests/integrate_test/cita_basic.sh
echo "5.2) byzantine test"
time ./tests/integrate_test/cita_byzantinetest.sh

echo "################################################################################"
echo "6) archive result"
now=$(date --iso-8601=minutes)
mkdir -p ${SOURCE_DIR}/../${now}_${BASHPID}
cp -rf ${SOURCE_DIR}/target/install  ${SOURCE_DIR}/../${now}_${BASHPID}/
cp -rf ${SOURCE_DIR}/target/*.log  ${SOURCE_DIR}/../${now}_${BASHPID}/

sed -i 's/\["sm2"\]/\["secp256k1"\]/g' share_libs/crypto/Cargo.toml
sed -i 's/\["sm3hash"\]/\["sha3hash"\]/g' share_libs/util/Cargo.toml
