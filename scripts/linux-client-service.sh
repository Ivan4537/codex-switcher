#!/usr/bin/env bash
set -euo pipefail
# A lingering user service starts before login. Use the desktop once it is available.
if [[ -n "${DISPLAY:-}" ]] && timeout 3 xdpyinfo >/dev/null 2>&1; then
    exec /usr/bin/codex-switcher
fi
exec /usr/bin/xvfb-run -a -s '-screen 0 1280x800x24 -nolisten tcp' /usr/bin/codex-switcher
