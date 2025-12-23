set -euo pipefail

BUILD_OUTPUT_FILE="$1"
COMMON_FILE="$2"
LIB_FILE="$3"
BIN_FILE="$4"
VAR_NAME="$5"
CRATE_NAME="${6:-}"
CRATE_VERSION="${7:-}"

# Extract shared rustc arguments.
RUSTC_FLAGS=$(sed -n "s/^cargo::\{0,1\}rustc-flags=\(.*\)/\1/p" "$BUILD_OUTPUT_FILE" | tr '\n' ' ')
RUSTC_CFG=$(sed -n "s/^cargo::\{0,1\}rustc-cfg=\(.*\)/--cfg \1/p" "$BUILD_OUTPUT_FILE" | tr '\n' ' ')
RUSTC_LINK_ARG=$(sed -n "s/^cargo::\{0,1\}rustc-link-arg=\(.*\)/-C link-arg=\1/p" "$BUILD_OUTPUT_FILE" | tr '\n' ' ')
RUSTC_LINK_LIB=$(sed -n "s/^cargo::\{0,1\}rustc-link-lib=\(.*\)/-l \1/p" "$BUILD_OUTPUT_FILE" | tr '\n' ' ')
RUSTC_LINK_SEARCH=$(sed -n "s/^cargo::\{0,1\}rustc-link-search=\(.*\)/-L \1/p" "$BUILD_OUTPUT_FILE" | tr '\n' ' ' | sort -u)

# Extract lib-specific arguments.
RUSTC_LINK_ARG_LIB=$(sed -n "s/^cargo::\{0,1\}rustc-link-arg-lib=\(.*\)/-C link-arg=\1/p" "$BUILD_OUTPUT_FILE" | tr '\n' ' ')
RUSTC_CDYLIB_LINK_ARG=$(sed -n "s/^cargo::\{0,1\}rustc-cdylib-link-arg=\(.*\)/-C link-arg=\1/p" "$BUILD_OUTPUT_FILE" | tr '\n' ' ')

# Extract bin-specific arguments.
RUSTC_LINK_ARG_BINS=$(sed -n "s/^cargo::\{0,1\}rustc-link-arg-bins=\(.*\)/-C link-arg=\1/p" "$BUILD_OUTPUT_FILE" | tr '\n' ' ')

# Handle rustc-env with proper IFS to support spaces in values.
_OLDIFS="$IFS"
IFS=$'\n'
for env in $(sed -n "s/^cargo::\{0,1\}rustc-env=\(.*\)/\1/p" "$BUILD_OUTPUT_FILE"); do
  echo "export $env" >> "$COMMON_FILE"
done
IFS="$_OLDIFS"

# Add metadata environment variables for downstream crates.
if [ -n "$CRATE_NAME" ] && [ -n "$CRATE_VERSION" ]; then
  CRATENAME=$(echo "$CRATE_NAME" | sed -e "s/\(.*\)-sys$/\U\1/" -e "s/-/_/g")
  CRATEVERSION=$(echo "$CRATE_VERSION" | sed -e "s/[\.\+-]/_/g")

  grep -P "^cargo:(?!:?(rustc-|warning=|rerun-if-changed=|rerun-if-env-changed))" "$BUILD_OUTPUT_FILE" \
    | awk -F= "/^cargo::metadata=/ {  gsub(/-/, \"_\", \$2); print \"export \" toupper(\"DEP_$CRATENAME\" \"_\" \$2) \"=\" \"\\\"\"\$3\"\\\"\"; next }
                /^cargo:/ { sub(/^cargo::?/, \"\", \$1); gsub(/-/, \"_\", \$1); print \"export \" toupper(\"DEP_$CRATENAME\" \"_\" \$1) \"=\" \"\\\"\"\$2\"\\\"\"; print \"export \" toupper(\"DEP_$CRATENAME\" \"_$CRATEVERSION\" \"_\" \$1) \"=\" \"\\\"\"\$2\"\\\"\"; next }" >> "$COMMON_FILE" || true
fi

# Add shared rustc arguments.
cat >> "$COMMON_FILE" <<EOF
export ${VAR_NAME}="${RUSTC_FLAGS}${RUSTC_CFG}${RUSTC_LINK_ARG}${RUSTC_LINK_LIB}${RUSTC_LINK_SEARCH}"
EOF

# Create lib-specific file.
cat > "$LIB_FILE" <<EOF
source "$COMMON_FILE"
export ${VAR_NAME}="\${${VAR_NAME}}${RUSTC_LINK_ARG_LIB}${RUSTC_CDYLIB_LINK_ARG}"
EOF

# Create bin-specific file.
cat > "$BIN_FILE" <<EOF
source "$COMMON_FILE"
export ${VAR_NAME}="\${${VAR_NAME}}${RUSTC_LINK_ARG_BINS}"
EOF
