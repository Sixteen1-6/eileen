#!/usr/bin/env bash
# Runs a command inside a headless Linux desktop session: the Linux desktop lane of BUILD.md
# section 4.6 (docs/LINUX.md section 6), for .github/workflows/linux-desktop.yml. Usage, from the
# repository root on the runner, as the runner's user, who may use sudo:
#
#   bash .github/linux-desktop/session.sh <command> [<argument>...]
#
# It runs itself again for each of three steps, which EILEEN_SESSION_STEP names:
#   login     As the runner: starts the next step as a transient system service of the runner's
#             user with PAM's `login` stack, so pam_systemd registers a logind session on seat0,
#             of type wayland, as GDM does for GNOME. logind and polkit then treat the tests as a
#             user's local session, which a process outside every session is not.
#   bus       Inside that session: makes it seat0's active session, points the XDG folders at
#             fresh ones (the user's systemd instance has its own sockets in /run/user), writes
#             the PipeWire and portal configuration, and starts dbus-run-session, whose bus is the
#             session bus of every service below and of the tests.
#   services  On that bus: checks that bwrap starts glycin's sandbox, starts the compositor and
#             the services, waits up to 20 s for each to answer, prints what the session is, runs
#             the command, and stops what it started.
# The exit status is the command's, or 1 naming what did not come up.
#
# weston's headless backend offers no seat (no keyboard or pointer), which GTK warns about and
# notification-daemon, an X11 program, crashes on: it runs on an Xvfb display of its own.
#
# gdk-pixbuf loads every image through glycin, GTK's icons among them, and glycin runs each
# loader in a sandbox that bwrap makes. bwrap refuses to start in a process that holds any
# capability, and pam_systemd gives a regular user who logs in on a seat CAP_WAKE_ALARM in its
# ambient set, which systemd keeps for a unit with a PAM stack, so every program of the session
# would hold it. Then each loader exits at once, and GTK 3 aborts when an icon and then its
# fallback fail to load: notification-daemon did, on its first notification. So the session's
# unit leaves CAP_WAKE_ALARM out of its bounding set, and systemd raises no ambient capability
# that the bounding set leaves out. The services step checks that bwrap starts, before anything.
set -euo pipefail

script="$(realpath "${BASH_SOURCE[0]}")"
root="${RUNNER_TEMP:?RUNNER_TEMP must name a temporary folder}/eileen-session"
wayland=eileen-0
logs="${EILEEN_SESSION_LOGS:-$root/logs}"
pids=()
x11=none

# Retries a check until it passes, for up to 20 s; else ends the step, naming what did not come
# up and showing the end of each service's log.
await() {
  local what="$1" until=$((SECONDS + 20))
  shift
  until "$@" >/dev/null 2>&1; do
    if [ "$SECONDS" -ge "$until" ]; then
      echo "session.sh: $what did not come up within 20 s; the logs are in $logs" >&2
      tail -n 20 "$logs"/*.log >&2 2>/dev/null || true
      exit 1
    fi
    sleep 0.2
  done
}

session_is_active() {
  [ "$(loginctl show-session "$XDG_SESSION_ID" --property=Active --value)" = yes ]
}

# A sandbox as glycin makes one for an image loader: every namespace bwrap can unshare, /usr
# read-only and a merged /usr's links, in which /usr/bin/true runs. bwrap says on stderr why not.
sandbox() {
  bwrap --unshare-all --die-with-parent --ro-bind /usr /usr --symlink usr/lib /lib \
    --symlink usr/lib64 /lib64 --dev /dev /usr/bin/true
}

login() {
  local n=1 vt=()
  while [ -e "$root/logs/session-$n" ]; do n=$((n + 1)); done
  mkdir -p "$root/logs/session-$n"
  # logind gives a session on seat0 a VT number only where seat0 has VTs. From VT 8 up there is
  # no getty, and each session takes its own, so one still closing never holds the next one's.
  if [ "$(loginctl show-seat seat0 --property=CanTTY --value)" = yes ]; then
    vt=(--setenv=XDG_VTNR=$((7 + n)))
  fi
  # PATH goes by another name, since pam_env sets PATH from /etc/environment. The bounding set
  # leaves out CAP_WAKE_ALARM, which pam_systemd would give the session (the header says why).
  exec sudo -n systemd-run --pipe --wait --collect --quiet --same-dir --expand-environment=no \
    --uid="$(id -u)" --gid="$(id -g)" \
    --property=PAMName=login --property=RuntimeMaxSec=1500 \
    --property=CapabilityBoundingSet=~CAP_WAKE_ALARM \
    --setenv=EILEEN_SESSION_STEP=bus --setenv=EILEEN_SESSION_LOGS="$root/logs/session-$n" \
    --setenv=EILEEN_RUNNER_PATH="$PATH" \
    --setenv=CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}" \
    --setenv=RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}" \
    --setenv=CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}" \
    --setenv=CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-auto}" \
    --setenv=RUST_BACKTRACE="${RUST_BACKTRACE:-1}" \
    --setenv=RUNNER_TEMP="$RUNNER_TEMP" \
    --setenv=GITHUB_STEP_SUMMARY="${GITHUB_STEP_SUMMARY:-/dev/null}" \
    --setenv=XDG_SESSION_TYPE=wayland --setenv=XDG_SEAT=seat0 "${vt[@]}" \
    -- bash "$script" "$@" </dev/null
}

bus() {
  export PATH="$EILEEN_RUNNER_PATH"
  if [ -z "${XDG_SESSION_ID:-}" ]; then
    echo "session.sh: pam_systemd registered no logind session (XDG_SESSION_ID is unset)" >&2
    exit 1
  fi
  # A desktop's session is the active one on its seat, and polkit's defaults for a local session
  # differ by whether it is active.
  sudo -n loginctl activate "$XDG_SESSION_ID"
  await "the session's activation on seat0" session_is_active
  # Fresh folders, short enough for Unix socket paths (about 108 bytes).
  rm -rf "${root:?}/run" "${root:?}/home"
  mkdir -m 700 "$root/run"
  export XDG_RUNTIME_DIR="$root/run" XDG_CONFIG_HOME="$root/home/config"
  export XDG_DATA_HOME="$root/home/data" XDG_STATE_HOME="$root/home/state"
  export XDG_CACHE_HOME="$root/home/cache"
  mkdir -p "$XDG_CONFIG_HOME/pipewire/pipewire.conf.d" "$XDG_CONFIG_HOME/xdg-desktop-portal" \
    "$XDG_DATA_HOME" "$XDG_STATE_HOME" "$XDG_CACHE_HOME"
  # Set before the bus starts, so the services it starts find the compositor.
  export WAYLAND_DISPLAY="$wayland"
  unset DISPLAY
  cat >"$XDG_CONFIG_HOME/pipewire/pipewire.conf.d/50-eileen-null-sink.conf" <<'CONF'
context.objects = [
  { factory = adapter
    args = {
      factory.name     = support.null-audio-sink
      node.name        = "eileen-null-sink"
      node.description = "Eileen null sink"
      media.class      = "Audio/Sink"
      audio.position   = [ FL FR ]
    }
  }
]
CONF
  printf '[preferred]\ndefault=gtk\n' >"$XDG_CONFIG_HOME/xdg-desktop-portal/portals.conf"
  export EILEEN_SESSION_STEP=services
  exec dbus-run-session -- bash "$script" "$@"
}

# Starts a service in the background, with its own log.
start() {
  local name="$1"
  shift
  "$@" </dev/null >"$logs/$name.log" 2>&1 &
  pids+=("$!")
}

# Stops what `start` started: politely, then for good after 5 s.
stop() {
  local pid until=$((SECONDS + 5))
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  while [ "$SECONDS" -lt "$until" ] && running; do sleep 0.1; done
  for pid in "${pids[@]}"; do kill -KILL "$pid" 2>/dev/null || true; done
  wait 2>/dev/null || true
}

running() {
  local pid
  for pid in "${pids[@]}"; do kill -0 "$pid" 2>/dev/null && return 0; done
  return 1
}

session_call() { gdbus call --session --timeout 5 "$@"; }
session_get() {
  session_call --dest "$1" --object-path "$2" --method org.freedesktop.DBus.Properties.Get "$3" "$4"
}
session_ping() { busctl --user --timeout=5 call "$1" "$2" org.freedesktop.DBus.Peer Ping; }
system_get() { busctl --system --timeout=5 get-property "$@"; }

null_sink_is_default() {
  local sink
  sink="$(timeout 5 wpctl inspect @DEFAULT_AUDIO_SINK@)" &&
    [[ "$sink" == *'node.name = "eileen-null-sink"'* ]]
}

# The accessibility bus's address, from at-spi-bus-launcher, which starts the bus on the call.
a11y_address() {
  local reply
  reply="$(session_call --dest org.a11y.Bus --object-path /org/a11y/bus \
    --method org.a11y.Bus.GetAddress)" || return 1
  reply="${reply#\(\'}"
  reply="${reply%\',\)}"
  [ -n "$reply" ] && printf '%s\n' "$reply"
}

secret_kept() {
  printf 'eileen-ci' | timeout 5 secret-tool store --label='Eileen session check' eileen check &&
    [ "$(timeout 5 secret-tool lookup eileen check)" = eileen-ci ]
}

# One row of the summary's table, on one line.
row() {
  local value="${2//$'\n'/; }"
  printf '| %s | %s |\n' "$1" "${value//|/\\|}"
}

inventory() {
  local package
  printf '### The headless session, for: %s\n\n| What | Found |\n|---|---|\n' "$*"
  row OS "$(sed -n 's/^PRETTY_NAME="\(.*\)"$/\1/p' /etc/os-release), kernel $(uname -r)"
  row "logind session" "$(loginctl show-session "$XDG_SESSION_ID" --property=Id --property=Type \
    --property=Class --property=Seat --property=VTNr --property=Active 2>&1)"
  row seat0 "$(loginctl show-seat seat0 --property=CanTTY --property=CanGraphical 2>&1)"
  row cgroup "$(cut -d: -f3 /proc/self/cgroup)"
  row displays "Wayland $WAYLAND_DISPLAY (weston, no seat); X11 $x11 (Xvfb, notification-daemon's)"
  row "notification server" "$(session_call --dest org.freedesktop.Notifications \
    --object-path /org/freedesktop/Notifications \
    --method org.freedesktop.Notifications.GetServerInformation 2>&1)"
  row "MPRIS player" "$(session_get org.mpris.MediaPlayer2.mpv /org/mpris/MediaPlayer2 \
    org.mpris.MediaPlayer2.Player PlaybackStatus 2>&1)"
  row "UPower OnBattery" "$(system_get org.freedesktop.UPower /org/freedesktop/UPower \
    org.freedesktop.UPower OnBattery 2>&1)"
  row "power profile" "$(system_get org.freedesktop.UPower.PowerProfiles \
    /org/freedesktop/UPower/PowerProfiles org.freedesktop.UPower.PowerProfiles ActiveProfile 2>&1)"
  for package in systemd libpam-systemd polkitd dbus-daemon weston xvfb pipewire wireplumber \
    at-spi2-core xdg-desktop-portal xdg-desktop-portal-gtk notification-daemon bubblewrap \
    glycin-loaders gnome-keyring mpv mpv-mpris upower power-profiles-daemon; do
    row "$package" "$(dpkg-query --show --showformat="\${Version}" "$package" 2>&1)"
  done
  echo
}

services() {
  local said ambient
  trap stop EXIT
  if ! said="$(sandbox 2>&1)"; then
    ambient="$(sed -n 's/^CapAmb:[[:space:]]*//p' /proc/self/status)"
    echo "session.sh: bwrap cannot make glycin's sandbox, in which GTK loads its icons: $said" \
      "(the session's ambient capabilities: $ambient)" >&2
    exit 1
  fi
  start weston weston --backend=headless --shell=kiosk --socket="$wayland" --idle-time=0 \
    --no-config
  await "the compositor (weston)" test -S "$XDG_RUNTIME_DIR/$wayland"
  start pipewire pipewire
  start wireplumber wireplumber
  await "the null sink as the default sink (PipeWire and WirePlumber)" null_sink_is_default
  await "the accessibility bus (at-spi-bus-launcher)" a11y_address
  await "the accessibility registry (at-spi2-registryd)" busctl --address="$(a11y_address)" \
    --timeout=5 call org.a11y.atspi.Registry /org/a11y/atspi/registry org.freedesktop.DBus.Peer Ping
  # Xvfb writes its display number to fd 3 once it accepts clients.
  start x11 Xvfb -displayfd 3 -nolisten tcp -screen 0 1280x720x24 3>"$XDG_RUNTIME_DIR/x11-display"
  await "the X server for notification-daemon (Xvfb)" test -s "$XDG_RUNTIME_DIR/x11-display"
  x11=":$(cat "$XDG_RUNTIME_DIR/x11-display")"
  start notifications env GDK_BACKEND=x11 DISPLAY="$x11" \
    /usr/lib/notification-daemon/notification-daemon --replace
  await "the notification server (notification-daemon)" \
    gdbus wait --session --timeout 1 org.freedesktop.Notifications
  # The password on stdin creates the login keyring and unlocks it, so nothing prompts.
  printf 'eileen-ci' | gnome-keyring-daemon --foreground --unlock --components=secrets \
    >"$logs/keyring.log" 2>&1 &
  pids+=("$!")
  await "the Secret Service (GNOME Keyring)" \
    gdbus wait --session --timeout 1 org.freedesktop.secrets
  await "a secret kept in the login keyring" secret_kept
  await "the Inhibit portal (xdg-desktop-portal)" session_get org.freedesktop.portal.Desktop \
    /org/freedesktop/portal/desktop org.freedesktop.portal.Inhibit version
  await "the portal's GTK backend (xdg-desktop-portal-gtk)" \
    session_ping org.freedesktop.impl.portal.desktop.gtk /org/freedesktop/portal/desktop
  start mpris mpv --no-config --script=/usr/lib/mpv-mpris/mpris.so --pause --idle=yes --vo=null \
    --ao=pipewire --input-terminal=no 'av://lavfi:sine=frequency=440'
  await "the MPRIS player (mpv)" gdbus wait --session --timeout 1 org.mpris.MediaPlayer2.mpv
  await "UPower" system_get org.freedesktop.UPower /org/freedesktop/UPower org.freedesktop.UPower \
    OnBattery
  await "power-profiles-daemon" system_get org.freedesktop.UPower.PowerProfiles \
    /org/freedesktop/UPower/PowerProfiles org.freedesktop.UPower.PowerProfiles ActiveProfile
  inventory "$@" | tee -a "$GITHUB_STEP_SUMMARY"
  local status=0
  "$@" || status=$?
  exit "$status"
}

if [ "$#" -eq 0 ]; then
  echo "usage: bash .github/linux-desktop/session.sh <command> [<argument>...]" >&2
  exit 2
fi
case "${EILEEN_SESSION_STEP:-login}" in
login) login "$@" ;;
bus) bus "$@" ;;
services) services "$@" ;;
*)
  echo "session.sh: no step named ${EILEEN_SESSION_STEP}" >&2
  exit 2
  ;;
esac
