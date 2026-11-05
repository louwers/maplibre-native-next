#!/usr/bin/env bash
# Prints the memory in use and the three largest processes once a minute.
# CI runs it in the background of its Linux test steps: their log survives a
# runner that runs out of memory and loses contact with GitHub.
set -u
while sleep 60; do
  free -m | awk '/^Mem:/ { printf "memory: %s MiB used, %s MiB available;", $3, $7 }'
  ps -eo rss=,comm= --sort=-rss | head -3 | awk '{ printf " %s %d MiB", $2, $1 / 1024 }'
  echo
done
