#!/usr/bin/env bash

set -euox pipefail

# An end to end test that lifts an XFS file system from inside an EXT4 one.


# Build
cargo build --release

sudo -v

# Delete previous things
if [ -d ./testloop ]; then
    if [ -d ./testloop/inner ]; then
        sudo umount ./testloop/inner || true
    fi

    sudo umount ./testloop || true
fi

rm -f testloop.img

# Create new host FS
LENGTH=$((1024 * 1024 * 1024))

truncate -s $LENGTH ./testloop.img

mkfs.ext4 ./testloop.img

mkdir -p testloop
sudo mount -t ext4 ./testloop.img ./testloop

sudo chown $USER:$USER ./testloop/.

# Put some data on the host FS

cp -r ./target/release ./testloop/
sync
df -h ./testloop

# Crete new inner FS

truncate -s $LENGTH ./testloop/inner.img

mkfs.xfs ./testloop/inner.img

mkdir -p ./testloop/inner
sudo mount -t xfs -o discard ./testloop/inner.img ./testloop/inner

sudo chown $USER:$USER ./testloop/inner/.

# Move files from host FS to inner FS

mv ./testloop/release ./testloop/inner/
df -h ./testloop/inner

# Create mapping

sudo umount ./testloop/inner
fallocate --dig-holes ./testloop/inner.img
sudo umount ./testloop
sudo mount -t ext4 -o ro ./testloop.img ./testloop

sudo ./target/release/looplift scan ./testloop/inner.img ./testloop.img | gzip -cv > ./test_mapping.gz

sudo umount ./testloop

# Apply mapping

cat ./test_mapping.gz | gzip -dc | sudo ./target/release/looplift lift ./testloop.img

# Profit ??

xfs_repair -n ./testloop.img
sudo mount -t xfs ./testloop.img ./testloop

(diff -r ./target/release ./testloop/release && echo 'DIFF PASSED') || (echo 'DIFF FAILED' && exit 1)

echo "ALL TESTS PASS?!?"
