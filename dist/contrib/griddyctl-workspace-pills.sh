#!/bin/bash
# GriddyWM workspace pills script for Waybar custom/griddy-workspaces module.
#
# Outputs a JSON array of workspace pill objects:
#   [{"text":"0,0","class":"active"},{"text":"1,0","class":""},...]
#
# Usage in waybar config.jsonc:
#   "custom/griddy-workspaces": {
#       "exec": "~/.config/waybar/griddyctl-workspace-pills.sh",
#       "return-type": "json",
#       "interval": 1
#   }

set -euo pipefail

# Get workspace list as JSON and build pills.
griddyctl -j get workspaces | python3 -c "
import json, sys
ws = json.load(sys.stdin)
pills = []
for w in ws:
    label = f\"{w['col']},{w['row']}\"
    name  = w.get('name', label)
    cls   = 'active' if w.get('focused') else ('occupied' if w.get('windows') else '')
    pills.append({'text': name or label, 'class': cls, 'tooltip': label})
print(json.dumps(pills))
"
