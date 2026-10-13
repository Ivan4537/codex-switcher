#!/usr/bin/env bash
set -euo pipefail
# Route desktop launches through the same service, rather than starting a second proxy.
for variable in DISPLAY XAUTHORITY WAYLAND_DISPLAY XDG_SESSION_TYPE; do
    if [[ -n "${!variable:-}" ]]; then
        systemctl --user import-environment "$variable"
    fi
done
exec systemctl --user restart codex-switcher.service
