#!/bin/bash

set -e  # Exit immediately if a command exits with a non-zero status

## VARIABLES ##
TARGET_OVERLAY_DIR=${TARGET_OVERLAY_DIR:-"/home/retro/.steam/debian-installation/steamapps"}
LOWER_DIR=${LOWER_DIR:-"/overlayfs/steam-ro/"}
UPPER_DIR=${UPPER_DIR:-"/overlayfs/user/upper/"}
WORK_DIR=${WORK_DIR:-"/overlayfs/user/work/"}
###############

# Include the gow bash utils library for logging
source /opt/gow/bash-lib/utils.sh || { echo "Failed to source utils.sh"; exit 1; }

# Assign defaults if needed
PUID="${PUID:-1000}"
PGID="${PGID:-1000}"

gow_log "[OverlayFS-Entrypoint] Starting container setup..."
gow_log "[OverlayFS-Entrypoint] Permission UID:GID is ${PUID}:${PGID}"

# Function to handle cleanup
cleanup() {
    gow_log "[OverlayFS-Entrypoint] Container is shutting down..."
    # Any other cleanup tasks can go here
}

# Set trap for cleanup
trap cleanup EXIT

# Function to check and set permissions if needed
check_and_set_permissions() {
    local dir=$1
    local owner="${PUID}:${PGID}"
    
    gow_log "[OverlayFS-Entrypoint] Checking permissions for ${dir}..."
    
    if [ "$(stat -c '%U:%G' "${dir}")" != "${owner}" ]; then
        gow_log "[OverlayFS-Entrypoint] Updating ownership of ${dir} to ${owner}..."
        chown "${owner}" "${dir}"
        if [ $? -ne 0 ]; then
            gow_log "[OverlayFS-Entrypoint] Failed to set ownership on ${dir}"
            return 1
        fi
    else
        gow_log "[OverlayFS-Entrypoint] Ownership of ${dir} is already correct"
    fi

    # Check a random file/subdirectory in the directory
    local sample_item=$(find "${dir}" -maxdepth 1 | head -n 2 | tail -n 1)
    if [ -n "${sample_item}" ] && [ "$(stat -c '%U:%G' "${sample_item}")" != "${owner}" ]; then
        gow_log "[OverlayFS-Entrypoint] Updating ownership of contents in ${dir} to ${owner}..."
        chown -R "${owner}" "${dir}"
        if [ $? -ne 0 ]; then
            gow_log "[OverlayFS-Entrypoint] Failed to set ownership on contents of ${dir}"
            return 1
        fi
    else
        gow_log "[OverlayFS-Entrypoint] Ownership of contents in ${dir} appears to be correct"
    fi

    return 0
}

# Make sure the overlayfs directories and targeted overlay directory exist
gow_log "[OverlayFS-Entrypoint] Creating necessary directories..."
mkdir -p /overlayfs/user/{upper,work} || { gow_log "[OverlayFS-Entrypoint] Failed to create overlay directories"; exit 1; }
mkdir -p $TARGET_OVERLAY_DIR || { gow_log "[OverlayFS-Entrypoint] Failed to create target directory"; exit 1; }

# Check and set permissions of the directories
check_and_set_permissions "/overlayfs/user/upper" || exit 1
check_and_set_permissions "/overlayfs/user/work" || exit 1
check_and_set_permissions "/home/retro" || exit 1

gow_log "[OverlayFS-Entrypoint] Creating overlay mount..."

# Create the overlay
mount -t overlay overlay -o lowerdir=$LOWER_DIR,upperdir=$UPPER_DIR,workdir=$WORK_DIR $TARGET_OVERLAY_DIR || { gow_log "[OverlayFS-Entrypoint] Failed to mount overlay"; exit 1; }

# Launch the base image's entrypoint.sh which will handle starting steam
gow_log "[OverlayFS-Entrypoint] Launching base entrypoint.sh"
source /entrypoint.sh || { gow_log "[OverlayFS-Entrypoint] Failed to source entrypoint.sh"; exit 1; }
