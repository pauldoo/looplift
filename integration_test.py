#!/usr/bin/env python3

import hashlib
import os
import shutil
import sys
import uuid

# Source of some test files
test_data = "./target"

test_image = "./testloop.img"
test_dir = "./testloop"

image_filename = f"{str(uuid.uuid4())}.img"
test_image_inner = f"{test_dir}/{image_filename}"
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

    print("Optimizing and remounting read-only")
    remount_outer_ro()
    print("Computing content hashes (before state)")
    original_outer = content_hash(test_image)
    original_inner = content_hash(test_image_inner)


    obtain_mapping() # also unmounts

     # Dryrun.
    apply_mapping(True)
    hc = content_hash(test_image)
    assert hc == original_outer, f"Dry run should not alter image."

    # Real lift.
    apply_mapping(False)
    hc = content_hash(test_image)
    assert hc == original_inner, f"Actual lift should result in expected hash."

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
    execute(f"sudo rsync -a -x -H -A -X --sparse --remove-source-files --exclude {image_filename} {test_dir}/ {test_dir_inner}/")

def remount_outer_ro():
    execute(f"sudo umount {test_dir_inner}")
    execute(f"fallocate --dig-holes {test_image_inner}")
    execute(f"sudo mount -o remount,ro {test_dir}")

def content_hash(file):
    with open(file, "rb") as f:
        sha256 = hashlib.file_digest(f, "sha256").hexdigest()
    print(f"File {file} has content hash: {sha256}")
    return sha256

def obtain_mapping():
    print("Obtaiing looplift mapping")
    execute(f"bash -o pipefail -c \"{looplift_binary} scan {test_image_inner} {test_image} | gzip -c > {test_mapping}\"")
    execute(f"sudo umount {test_dir}")

def apply_mapping(dry_run):
    print("Looplift dry-run" if dry_run else "Looplift for real")
    execute(f"bash -o pipefail -c \"zcat {test_mapping} | {looplift_binary} lift {"" if dry_run else "--dry-run false "}{test_image}\"")

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
