# Materialize a golden case into a work directory: go.mod + the sources listed
# in the case's sources.txt, copied from their canonical location in the repo.
#
# Sourced by compat/golden/run.sh and compat/fix/run.sh. The two tiers ask
# different questions of the same 193 cases — the golden tier compares what the
# tools *say*, the fix tier what they *write* — but "what is a case" has to be
# one answer in one place, or a case that the golden tier reads with three files
# gets linted by the fix tier with two.
#
#   materialize_case <name> <case_dir> <work_dir> <repo_root>

materialize_case() {
  local name="$1" case_dir="$2" work="$3" root="$4"
  rm -rf "$work"
  mkdir -p "$work"
  cp "$case_dir/go.mod" "$work/go.mod"
  while IFS= read -r raw || [[ -n "$raw" ]]; do
    local line dest src
    line="${raw%%#*}"
    line="$(echo "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
    [[ -z "$line" ]] && continue
    # The two columns are separated by a run of two or more spaces, not by a
    # single one: either path may itself contain a space (revive's
    # filename-format fixture is literally named "bad file.go").
    if [[ "$line" =~ ^(.*[^[:space:]])[[:space:]][[:space:]]+(.+)$ ]]; then
      dest="${BASH_REMATCH[1]}"; src="${BASH_REMATCH[2]}"
    else
      echo "error: $name: sources.txt needs two or more spaces between the columns: $raw" >&2
      return 1
    fi
    if [[ ! -f "$root/$src" ]]; then
      echo "error: $name: missing source $src" >&2
      return 1
    fi
    mkdir -p "$(dirname "$work/$dest")"
    cp "$root/$src" "$work/$dest"
  done <"$case_dir/sources.txt"
}

# Read a case's optional `env` file into the caller's `case_env` array (and
# `case_goos` / `case_goarch` when it sets them). Applied to *both* tools: a
# check that returns early unless the word size is 4 is unreachable on the host
# arch and cannot be compared at all without it.
#
#   read_case_env <name> <case_dir>
read_case_env() {
  local name="$1" case_dir="$2" raw line
  case_env=()
  case_goos=""
  case_goarch=""
  [[ -f "$case_dir/env" ]] || return 0
  while IFS= read -r raw || [[ -n "$raw" ]]; do
    line="${raw%%#*}"
    line="$(echo "$line" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
    [[ -z "$line" ]] && continue
    if [[ "$line" != *=* ]]; then
      echo "error: $name: env line is not KEY=VALUE: $raw" >&2
      return 1
    fi
    case_env+=("$line")
    [[ "$line" == GOOS=* ]] && case_goos="${line#GOOS=}"
    [[ "$line" == GOARCH=* ]] && case_goarch="${line#GOARCH=}"
  done <"$case_dir/env"
}
