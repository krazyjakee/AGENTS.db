#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to display usage
usage() {
    echo "Usage: $0 <new-version>"
    echo "Example: $0 0.2.0"
    echo ""
    echo "This script will update the version in:"
    echo "  - All Cargo.toml files in crates/"
    echo "  - crates/agentsdb-cli/src/cli.rs about string"
    exit 1
}

# Check if version argument is provided
if [ $# -ne 1 ]; then
    usage
fi

NEW_VERSION=$1

# Validate version format (basic semver check)
if ! [[ $NEW_VERSION =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9]+)?$ ]]; then
    echo -e "${RED}Error: Invalid version format. Use semver format (e.g., 0.2.0 or 0.2.0-beta)${NC}"
    exit 1
fi

# Get current version from agentsdb-cli Cargo.toml
CURRENT_VERSION=$(grep -m1 '^version = ' crates/agentsdb-cli/Cargo.toml | sed 's/version = "\(.*\)"/\1/')

if [ -z "$CURRENT_VERSION" ]; then
    echo -e "${RED}Error: Could not determine current version${NC}"
    exit 1
fi

echo -e "${YELLOW}Current version: ${CURRENT_VERSION}${NC}"
echo -e "${YELLOW}New version: ${NEW_VERSION}${NC}"
echo ""

# Confirm with user
read -p "Proceed with version bump? (y/n) " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Aborted."
    exit 0
fi

echo ""
echo "Updating versions..."

# Portable in-place edit helper (works on macOS and Linux).
perl_inplace() {
    local expr="$1"
    local file="$2"
    perl -pi -e "$expr" "$file"
}

# Update all Cargo.toml files in crates/
for cargo_file in crates/*/Cargo.toml; do
    if [ -f "$cargo_file" ]; then
        echo "  - Updating $cargo_file"
        perl_inplace "s/^version = \\\".*\\\"/version = \\\"$NEW_VERSION\\\"/" "$cargo_file"
    fi
done

# Update the CLI about string
CLI_FILE="crates/agentsdb-cli/src/cli.rs"
if [ -f "$CLI_FILE" ]; then
    echo "  - Updating $CLI_FILE"
    perl_inplace "s/(about = \\\"AGENTS\\.db tooling \\(v)[^\\)]*(\\)\\\")/\\\${1}$NEW_VERSION\\\${2}/" "$CLI_FILE"
else
    echo -e "${RED}Error: $CLI_FILE not found${NC}"
    exit 1
fi

echo ""
echo -e "${GREEN}✓ Version bumped from ${CURRENT_VERSION} to ${NEW_VERSION}${NC}"
echo ""
echo "Changed files:"
git diff --name-only crates/*/Cargo.toml "$CLI_FILE" 2>/dev/null || echo "  (git not available or no changes detected)"
echo ""
echo "Next steps:"
echo "  1. Review changes: git diff"
echo "  2. Test build: cargo build -p agentsdb-cli --features all-embedders"
echo "  3. Commit: git commit -am \"Bump version to ${NEW_VERSION}\""
echo "  4. Tag: git tag v${NEW_VERSION}"
