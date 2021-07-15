#!/bin/bash

killall -9 rb-node

set -e

clear

cargo +nightly-2020-08-23 build --release -p rb-node

authorities=(alice bob charlie dave eve ferdie)
# authorities=("${authorities[@]::$1}")

for i in ${!authorities[@]}; do
  auth=${authorities[$i]}
  ./target/debug/rb-node purge-chain --base-path /tmp/$auth --chain local -y
done

for i in ${!authorities[@]}; do
  auth=${authorities[$i]}
  ./target/debug/rb-node \
    --validator \
    --chain local \
    --base-path /tmp/$auth \
    --$auth \
    --ws-port $(expr 9944 + $i) \
    --port $(expr 30334 + $i) \
    --rpc-port $(expr 9934 + $i) \
    --execution Native \
    2> $auth.log   & \
done
