#!/bin/bash
set -e
cd "$(dirname "$0")"
swiftc -O -o NDIMixerMonitor NDIMixerMonitor.swift -framework AppKit
echo "Built: monitor/NDIMixerMonitor"
