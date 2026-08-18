#!/usr/bin/env bash
# Inside-out code-sign a built "Codex Manager.app", including the nested
# vendored Sparkle `BinaryDelta` helper, then verify the whole bundle.
#
# Why this exists: `tauri build` copies our helper into Contents/Resources/ and
# signs the OUTER .app, but a Mach-O helper must be signed FIRST (inside-out) or
#   * `codesign --verify --deep --strict` rejects the bundle, and
#   * notarization rejects an adhoc-signed nested binary inside a Developer ID app.
# For distribution we therefore build WITHOUT a tauri signingIdentity and sign
# here: nested helper → outer app → verify (→ then notarize/staple separately).
#
# Identity resolution (first non-empty wins):
#   $2 arg  →  $CAM_SIGN_IDENTITY  →  $APPLE_SIGNING_IDENTITY  →  "-" (adhoc)
# "-" (adhoc) is correct for local/dev builds. A real "Developer ID Application:
# … (TEAMID)" identity additionally enables hardened runtime + a secure timestamp
# and (for the outer app) the entitlements file.
set -euo pipefail

APP="${1:-}"
IDENTITY="${2:-${CAM_SIGN_IDENTITY:-${APPLE_SIGNING_IDENTITY:--}}}"
ENTITLEMENTS="${CAM_ENTITLEMENTS:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/src-tauri/entitlements.macos.plist}"

[[ -n "$APP" ]] || { echo "usage: sign-macos-app.sh <path-to-.app> [identity]" >&2; exit 2; }
[[ -d "$APP" ]] || { echo "no such app bundle: $APP" >&2; exit 1; }
[[ "$(uname -s)" == "Darwin" ]] || { echo "macOS only" >&2; exit 1; }

log() { printf '\033[36m[sign-macos]\033[0m %s\n' "$*"; }

base=(--force --sign "$IDENTITY")
app_opts=("${base[@]}")
helper_opts=("${base[@]}")
if [[ "$IDENTITY" != "-" ]]; then
  helper_opts+=(--options runtime --timestamp)
  app_opts+=(--options runtime --timestamp)
  if [[ -f "$ENTITLEMENTS" ]]; then
    app_opts+=(--entitlements "$ENTITLEMENTS")
    log "Developer ID identity + hardened runtime + entitlements (${ENTITLEMENTS##*/})"
  else
    log "Developer ID identity + hardened runtime (no entitlements file at $ENTITLEMENTS)"
  fi
else
  log "adhoc signing (local/dev build)"
fi

# Resolve the main executable so we sign it via the bundle (step 2), not as a
# helper.
main_exe="Contents/MacOS/$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$APP/Contents/Info.plist" 2>/dev/null || echo '')"

# 1) Sign every nested Mach-O FIRST (inside-out): the vendored BinaryDelta plus
#    any other helper / dylib / secondary executable the bundle carries. Doing
#    this generically keeps distribution signing correct even when extra binaries
#    land in the bundle — each needs its own hardened-runtime + timestamp seal or
#    notarization rejects the whole app.
saw_delta=0
while IFS= read -r f; do
  rel="${f#"$APP"/}"
  [[ "$rel" == "$main_exe" ]] && continue
  if file -b "$f" | grep -q "Mach-O"; then
    [[ "$rel" == *BinaryDelta ]] && saw_delta=1
    log "sign nested: $rel"
    codesign "${helper_opts[@]}" "$f"
  fi
done < <(find "$APP/Contents" -type f 2>/dev/null)
[[ "$saw_delta" == 1 ]] || log "note: no BinaryDelta helper in bundle — delta updates will fall back to full."

# 2) Sign the outer app last so its seal covers the freshly-signed helper.
log "sign app: ${APP##*/}"
codesign "${app_opts[@]}" "$APP"

# 3) Verify the whole bundle, helper included.
log "verify --deep --strict …"
codesign --verify --deep --strict --verbose=2 "$APP"
if [[ "$IDENTITY" != "-" ]]; then
  log "spctl assess (Gatekeeper) …"
  spctl -a -t exec -vv "$APP" || true
fi
log "OK — bundle verified."
