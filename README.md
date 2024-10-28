# Looplift

Looplift is a CLI tool that "lifts" files from within a filesystem, directly onto the block device hosting the filesystem.  This can be used to facilitate filesystem conversions or to migrate disk images stored on a filesystem to the device hosting that same filesystem.

# Example usage

## Filesystem conversion

Prerequisite: The source filesystem must support FIEMAP.

1. Create a new sparse file within the existing to-be-converted filesystem.  If the host FS supports transparent compression or encryption, it must be disabled for this file.
2. Format the sparse file with the target filesystem type, and mount (recommend to include `discard` option).
3. Move files from the original filesystem to the inner target filesystem.
4. Unmount the target filesystem, and remount the original filesystem read-only.
5. Perform the looplift "scan" step, store the output report file somewhere outside either filesystem.  The report should be small and compress easily.
6. Unmount the original filesystem.
7. Perform the looplift "lift" step with.
8. Mount the device, it should now be the target filesystem.
9. (Optional) run `fstrim`.

Check the `integration_test.py` script which exercises this end-to-end.

## Promoting a raw VM disk image

Similar to above, a raw disk image (e.g from a VM) can be promoted to the device hosting the FS.
