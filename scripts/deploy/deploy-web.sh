#!/usr/bin/env bash
# Build the page with the Fly origin and upload that build to Vercel.
# Vercel must not rebuild it: a rebuild would drop VITE_API_BASE.
set -euo pipefail

APP="${FLY_APP:-}"
ORIGIN="${VACA_API_ORIGIN:-}"
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

if [[ -z "$APP" || -z "$ORIGIN" ]]; then
  echo "Set FLY_APP and VACA_API_ORIGIN." >&2
  exit 1
fi
if [[ "$ORIGIN" != "https://${APP}.fly.dev" ]]; then
  echo "VACA_API_ORIGIN must be https://${APP}.fly.dev with no path or trailing slash." >&2
  exit 1
fi
for name in VERCEL_TOKEN VERCEL_ORG_ID VERCEL_PROJECT_ID; do
  if [[ -z "${!name:-}" ]]; then
    echo "Set ${name}." >&2
    exit 1
  fi
done

cd "$ROOT/web"
export VITE_API_BASE="$ORIGIN"
# Shows the fake-mint notice at the top of every page of the public demo.
export VITE_PUBLIC_DEMO=1
npm ci
npx --yes vercel@latest pull --yes --environment=production
npx --yes vercel@latest build --prod
npx --yes vercel@latest deploy --prebuilt --prod --yes
echo "Page deploy finished. VACA_CORS_ORIGIN on Fly must be this page's origin."
