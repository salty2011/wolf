#!/bin/bash

## VARIABLES ##
TARGET_OVERLAY_DIR=/home/retro/.steam/debian-installation/steamapps
###############

# Make sure the overlayfs directorys and targeted overlay directory exist
mkdir -p /overlayfs/user/{upper,work}
mkdir -p $TARGET_OVERLAY_DIR
chown 1000:1000 -R overlayfs/user/{upper,work}

chown 1000:1000 -R /home/retro/.steam

# Create the overlay using:
# 'lowerdir' is the readonly steamapps directory we defined in /etc/wolf/cfg/config.toml
# 'upperdir' is the persistent "read & write" directory
# 'workdir' seems to be like a cache for files that are in a in-between state? Its required reguardless and needs to be not a subfolder of 'upperdir'
# finally, the final arg '/home/retro/.steam/debian-installation/steamapps' is the targeted overlay directory we want the overlay to exist at
mount -t overlay overlay -o lowerdir=/overlayfs/steam-ro/,upperdir=/overlayfs/user/upper/,workdir=/overlayfs/user/work/ $TARGET_OVERLAY_DIR

source /entrypoint.sh