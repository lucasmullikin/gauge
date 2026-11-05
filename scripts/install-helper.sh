#!/bin/sh
# Installs the root fan helper. Usage: install-helper.sh UID PATH_TO_GAUGE_FAND (run as root)
set -eu
allow_uid=$1
source_binary=$2
case "$allow_uid" in
  ''|*[!0-9]*) echo "bad uid" >&2; exit 2 ;;
esac
if ! "$source_binary" --check; then
  echo "Fan control isn't supported on this Mac, so the helper wasn't installed." >&2
  exit 3
fi
label=com.gauge.fand
target=/Library/PrivilegedHelperTools/$label
plist=/Library/LaunchDaemons/$label.plist
launchctl bootout "system/$label" 2>/dev/null || true
install -d -o root -g wheel -m 755 /Library/PrivilegedHelperTools
install -o root -g wheel -m 755 "$source_binary" "$target"
# A root-owned copy, so uninstalling never runs a file a normal process could have changed.
install -o root -g wheel -m 755 "$(dirname "$0")/uninstall-helper.sh" "$target-uninstall"
cat > "$plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>$label</string>
  <key>ProgramArguments</key>
  <array>
    <string>$target</string>
    <string>--allow-uid</string>
    <string>$allow_uid</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>
  <key>StandardErrorPath</key><string>/var/log/gauge-fand.log</string>
</dict>
</plist>
PLIST
chown root:wheel "$plist"
chmod 644 "$plist"
launchctl bootstrap system "$plist"
