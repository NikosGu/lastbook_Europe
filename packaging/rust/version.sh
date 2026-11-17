#!/bin/bash
# The app's version, from the workspace's Cargo.toml.
sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$(cd "$(dirname "$0")/../.." && pwd)/Cargo.toml" | head -1
