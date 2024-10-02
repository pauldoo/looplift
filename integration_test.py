#!/usr/bin/env python3

import hashlib
import os
import shutil
import sys

# Source of some test files
test_data = "./target"

test_image = "./testloop.img"
test_dir = "./testloop"

test_image_inner = f"{test_dir}/inner.img"
test_dir_inner = f"{test_dir}/inner"

test_mapping = "test_mapping.gz"

user = os.environ.get('USER')

looplift_binary = "./target/release/looplift"

image_length = 1500 * 1024**2


def main():
    """An end to end test.

    We create a host ext4 filesystem, and put some test files onto it.

    Within that FS we create a new loopback XFS filesystem, and move the
    test files from the ext4 to XFS filesystem.

    We then use looplift commands to "lift" that nested loop device file
    to the outer file, "promoting" the XFS filesystem to the host FS device.
    """

    print("Starting test")
    build()
    cleanup()

    create_outer_fs("ext4")
    copy_test_data()
    validate_data()

    create_inner_fs("xfs")
    move_test_data_to_inner()

    remount_outer_ro()
    hc1 = obtain_mapping()
    hc2 = apply_mapping()
    assert hc1 == hc2, f"Hash codes should match {hc1} {hc2}"

    mount_promoted_fs("xfs")

    validate_data()

    print("Done")

def build():
    execute("cargo build --release")

def cleanup():
    print("Cleaning up")
    if os.path.isdir(test_dir):
        if os.path.isdir(test_dir_inner):
            execute(f"sudo umount {test_dir_inner} || true")
        execute(f"sudo umount {test_dir} || true")
        os.rmdir(test_dir)
    os.remove(test_image)

def create_outer_fs(fs_type):
    print("Creating initial outer FS")
    execute(f"truncate -s {image_length} {test_image}")
    execute(f"mkfs.{fs_type} -q {test_image}")
    os.makedirs(test_dir)
    execute(f"sudo mount -t {fs_type} {test_image} {test_dir}")
    execute(f"sudo chown {user}:{user} {test_dir}/.")

def create_inner_fs(fs_type):
    print("Creating inner FS")
    execute(f"truncate -s {image_length} {test_image_inner}")
    execute(f"mkfs.{fs_type} -q {test_image_inner}")
    os.makedirs(test_dir_inner)
    execute(f"sudo mount -t {fs_type} {test_image_inner} {test_dir_inner}")
    execute(f"sudo chown {user}:{user} {test_dir_inner}/.")

def copy_test_data():
    print("Copying initial test data")
    shutil.copytree(test_data, f"{test_dir}/data")
    os.sync()

def move_test_data_to_inner():
    print("Moving test data from outer to inner FS")
    shutil.move(f"{test_dir}/data", f"{test_dir_inner}/data")

def remount_outer_ro():
    execute(f"sudo umount {test_dir_inner}")
    execute(f"fallocate --dig-holes {test_image_inner}")
    execute(f"sudo mount -o remount,ro {test_dir}")

def obtain_mapping():
    with open(test_image_inner, "rb") as f:
        sha256 = hashlib.file_digest(f, "sha256").hexdigest()

    execute(f"{looplift_binary} scan {test_image_inner} {test_image} | gzip -c > {test_mapping}")
    execute(f"sudo umount {test_dir}")
    return sha256

def apply_mapping():
    execute(f"zcat {test_mapping} | {looplift_binary} lift {test_image}")
    with open(test_image, "rb") as f:
        sha256 = hashlib.file_digest(f, "sha256").hexdigest()
    return sha256

def mount_promoted_fs(fs_type):
    execute(f"sudo mount -t {fs_type} {test_image} {test_dir}")

def validate_data():
    execute(f"diff -r {test_data} {test_dir}/data")

def execute(cmd):
    print(f"> {cmd}")
    code = os.system(cmd)
    assert code == 0, f"Command failed with exit code {code}: {cmd}"

if __name__ == '__main__':
    sys.exit(main())
