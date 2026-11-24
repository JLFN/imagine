#!/usr/bin/env bash
# tools/recipe-check.sh — verify the model files a recipe describes.
#
# Two jobs, both offline except where a download is asked for:
#
#   tools/recipe-check.sh                 check every file the recipe names
#   tools/recipe-check.sh --profile NAME  check only one profile's files
#   tools/recipe-check.sh --download NAME download one file by its id
#   tools/recipe-check.sh --schema        check the recipe against its schema
#
# A file passes when its size and its SHA-256 match the recipe. A missing file
# is reported as missing, never as a pass, and a mismatch always means download
# again rather than proceed.
#
# The recipe is recipe.json at the project root; set RECIPE to use another.

set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RECIPE="${RECIPE:-$ROOT/recipe.json}"
SCHEMA="$ROOT/recipe.schema.json"

usage() {
  sed -n '2,18p' "$0" | sed 's/^# \{0,1\}//'
}

if [[ ! -f "$RECIPE" ]]; then
  echo "no recipe at $RECIPE" >&2
  exit 2
fi

command -v python3 >/dev/null 2>&1 || { echo "python3 is required" >&2; exit 2; }

MODE="check"
TARGET=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile) MODE="profile"; TARGET="${2:-}"; shift 2 ;;
    --download) MODE="download"; TARGET="${2:-}"; shift 2 ;;
    --schema) MODE="schema"; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

python3 - "$RECIPE" "$SCHEMA" "$MODE" "$TARGET" <<'PY'
import hashlib
import json
import os
import subprocess
import sys

recipe_path, schema_path, mode, target = sys.argv[1:5]

with open(recipe_path, encoding="utf-8") as f:
    recipe = json.load(f)

components = recipe.get("components", {})
profiles = recipe.get("profiles", {})

def all_files():
    for group, entries in components.items():
        for entry in entries:
            yield group, entry

def find(component_id):
    for group, entry in all_files():
        if entry.get("id") == component_id:
            return group, entry
    return None, None

def selected_files():
    if mode != "profile":
        return list(all_files())
    if target not in profiles:
        raise SystemExit(f"no profile named '{target}' in the recipe")
    profile = profiles[target]
    wanted = [profile["diffusion"], profile["encoder"], profile["vae"], *profile.get("adapters", [])]
    out = []
    for cid in wanted:
        group, entry = find(cid)
        if entry is None:
            raise SystemExit(f"profile '{target}' names '{cid}', which the recipe does not define")
        out.append((group, entry))
    return out

def sha256_of(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()

def check_schema():
    # A dependency-free structural check of the required rules in the schema:
    # every file has an id, a path, a size, and a 64-hex checksum, and every
    # profile names components that exist. This is not a general validator; it
    # is the subset this recipe relies on, stated plainly so it cannot rot.
    problems = []
    if recipe.get("recipe_version") != 1:
        problems.append("recipe_version must be 1")
    ids = {}
    for group, entry in all_files():
        cid = entry.get("id")
        if not cid:
            problems.append(f"{group}: a file has no id")
            continue
        if cid in ids:
            problems.append(f"duplicate id '{cid}'")
        ids[cid] = group
        for key in ("file", "size_bytes", "sha256", "source"):
            if key not in entry:
                problems.append(f"{cid}: missing {key}")
        digest = entry.get("sha256", "")
        if len(digest) != 64 or any(c not in "0123456789abcdef" for c in digest):
            problems.append(f"{cid}: sha256 is not 64 lowercase hex characters")
        size = entry.get("size_bytes", 0)
        if not isinstance(size, int) or size <= 0:
            problems.append(f"{cid}: size_bytes must be a positive integer")
        src = entry.get("source", {})
        for key in ("host", "repo", "path"):
            if key not in src:
                problems.append(f"{cid}: source is missing {key}")
        if "/" not in src.get("repo", ""):
            problems.append(f"{cid}: source repo should be owner/name")
        sched = entry.get("schedule")
        if sched is not None and len(sched) < 2:
            problems.append(f"{cid}: a schedule needs at least two values")
    for name, profile in profiles.items():
        for key in ("diffusion", "encoder", "vae"):
            cid = profile.get(key)
            if cid not in ids:
                problems.append(f"profile {name}: {key} names unknown id '{cid}'")
        for cid in profile.get("adapters", []):
            if cid not in ids:
                problems.append(f"profile {name}: adapter names unknown id '{cid}'")
    if problems:
        for p in problems:
            print(f"FAIL  {p}")
        return 1
    print(f"PASS  {recipe_path} matches the rules its schema states")
    print(f"      {len(ids)} files, {len(profiles)} profiles")
    return 0

def check_files():
    failures = 0
    rows = []
    for group, entry in selected_files():
        rel = entry["file"]
        path = rel if os.path.isabs(rel) else os.path.join(os.path.dirname(recipe_path), rel)
        expected_size = entry["size_bytes"]
        expected_hash = entry["sha256"]
        if not os.path.exists(path):
            print(f"MISSING  {rel}")
            print(f"         get it from {entry['source']['repo']} at {entry['source']['path']}")
            failures += 1
            continue
        actual_size = os.path.getsize(path)
        if actual_size != expected_size:
            print(f"FAIL  {rel}: {actual_size} bytes, expected {expected_size}")
            failures += 1
            continue
        actual_hash = sha256_of(path)
        if actual_hash != expected_hash:
            print(f"FAIL  {rel}: checksum differs")
            print(f"         expected {expected_hash}")
            print(f"         actual   {actual_hash}")
            failures += 1
            continue
        print(f"OK    {rel}")
        rows.append((group, entry.get("quantization", ""), actual_size))
    if failures:
        print(f"\n{failures} file(s) did not verify. Re-download a mismatch; never proceed past one.")
        return 1
    print(f"\nall {len(rows)} file(s) verified")
    return 0

def download():
    group, entry = find(target)
    if entry is None:
        print(f"no file with id '{target}' in the recipe", file=sys.stderr)
        return 2
    src = entry["source"]
    rel = entry["file"]
    path = rel if os.path.isabs(rel) else os.path.join(os.path.dirname(recipe_path), rel)
    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    url = src.get("url") or f"https://huggingface.co/{src['repo']}/resolve/{src.get('revision', 'main')}/{src['path']}"
    print(f"fetching {url}")
    result = subprocess.run(["curl", "-L", "--fail", "--retry", "3", "-C", "-", "-o", path, url])
    if result.returncode != 0:
        print("download failed", file=sys.stderr)
        return result.returncode
    actual_size = os.path.getsize(path)
    actual_hash = sha256_of(path)
    if actual_size != entry["size_bytes"] or actual_hash != entry["sha256"]:
        print(f"downloaded file does not match the recipe: {actual_size} bytes, sha256 {actual_hash}")
        return 1
    print(f"verified {path}")
    return 0

if mode == "schema":
    sys.exit(check_schema())
if mode == "download":
    sys.exit(download())
sys.exit(check_files())
PY
