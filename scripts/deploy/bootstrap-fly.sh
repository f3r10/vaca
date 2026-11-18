#!/usr/bin/env bash
# Create the Fly app and the SQLite volume. Does not set secrets and does not deploy.
set -euo pipefail

APP="${FLY_APP:-}"
REGION="${FLY_REGION:-iad}"

if [[ -z "$APP" ]]; then
  echo "Set FLY_APP to the Fly app name." >&2
  exit 1
fi
if [[ ! "$APP" =~ ^[a-z0-9][a-z0-9-]{1,30}$ ]]; then
  echo "FLY_APP must be a Fly app name: lowercase letters, digits, and hyphens." >&2
  exit 1
fi
if [[ "$REGION" != "iad" ]]; then
  echo "FLY_REGION must be iad, the primary_region in fly.toml. Change both together." >&2
  exit 1
fi

if command -v flyctl >/dev/null 2>&1; then
  FLY=(flyctl)
elif command -v fly >/dev/null 2>&1; then
  FLY=(fly)
else
  echo "flyctl is not on PATH." >&2
  exit 1
fi

if "${FLY[@]}" status --app "$APP" >/dev/null 2>&1; then
  echo "Fly app ${APP} already exists."
else
  "${FLY[@]}" apps create "$APP"
fi

volumes="$("${FLY[@]}" volumes list --app "$APP" --json)"
if printf '%s' "$volumes" | node --input-type=module -e '
  let raw = "";
  for await (const chunk of process.stdin) raw += chunk;
  let rows;
  try {
    rows = raw.trim() ? JSON.parse(raw) : [];
  } catch {
    console.error("Could not read the volume list.");
    process.exit(1);
  }
  if (!Array.isArray(rows)) {
    console.error("Could not read the volume list.");
    process.exit(1);
  }
  const found = rows.find((row) => row.name === "vaca_data");
  if (!found) process.exit(2);
  if (found.region !== "iad") {
    console.error("Volume vaca_data is in " + found.region + ", not iad.");
    process.exit(1);
  }
'; then
  echo "Volume vaca_data already exists in iad."
else
  status=$?
  if [[ "$status" -eq 1 ]]; then
    exit 1
  fi
  "${FLY[@]}" volumes create vaca_data --app "$APP" --region "$REGION" --size 1 --yes
fi

echo "App ${APP} has a volume. Secrets are still unset. Run scripts/deploy/set-fly-secrets.sh next."
