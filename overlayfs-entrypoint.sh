#!/bin/bash

## VARIABLES ##

OverlayGuestMountPoint=.steam/debian-installation/steamapps/

###############



mkdir -p /overlayfs/user/{upper,work}
mount -t overlay overlay -o lowerdir=/overlayfs/steam-ro/,upperdir=/overlayfs/user/upper/,workdir=/overlayfs/user/work/ /home/retro/$OverlayGuestMountPoint

source /entrypoint.sh
