#!/bin/sh
# Initialize Memory Bank for Cursor IDE in a project directory
# Usage: sh /path/to/memory-bank-system/.cursor/init-memory-bank.sh [--force] [target_dir]

set -e

FORCE=false
if [ "$1" = "--force" ]; then
    FORCE=true
    shift
fi

TARGET="${1:-.}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
TEMPLATE_DIR="$REPO_DIR/.claude/memory-bank-template"

echo "Initializing Memory Bank (Cursor IDE) in: $(cd "$TARGET" && pwd)"
if [ "$FORCE" = true ]; then
    echo "  (--force: overwriting existing files)"
fi

# Create memory-bank directory structure
mkdir -p "$TARGET/memory-bank"/{creative,review,integration,validation,reflection,archive,security}

# Copy template files
for tmpl in "$TEMPLATE_DIR"/*.md; do
    basename=$(basename "$tmpl")
    dest="$TARGET/memory-bank/$basename"
    if [ "$FORCE" = true ] || [ ! -f "$dest" ]; then
        cp "$tmpl" "$dest"
        echo "  Created: memory-bank/$basename"
    else
        echo "  Skipped: memory-bank/$basename (already exists)"
    fi
done

# Create .cursor directory
mkdir -p "$TARGET/.cursor"

# Copy commands
if [ -d "$SCRIPT_DIR/commands" ]; then
    if [ "$FORCE" = true ] || [ ! -d "$TARGET/.cursor/commands" ]; then
        rm -rf "$TARGET/.cursor/commands"
        cp -r "$SCRIPT_DIR/commands" "$TARGET/.cursor/commands"
        echo "  Created: .cursor/commands/ (van, plan, creative, build, reflect, archive)"
    else
        echo "  Skipped: .cursor/commands/ (already exists)"
    fi
else
    echo "  Skipped: .cursor/commands/ (source not found)"
fi

# Copy agents (mb-* subagents used by the /orchestrate command)
if [ -d "$SCRIPT_DIR/agents" ]; then
    if [ "$FORCE" = true ] || [ ! -d "$TARGET/.cursor/agents" ]; then
        rm -rf "$TARGET/.cursor/agents"
        cp -r "$SCRIPT_DIR/agents" "$TARGET/.cursor/agents"
        echo "  Created: .cursor/agents/ (mb-* stage subagents for /orchestrate)"
    else
        echo "  Skipped: .cursor/agents/ (already exists)"
    fi
else
    echo "  Skipped: .cursor/agents/ (source not found)"
fi

# Copy rules
if [ -d "$SCRIPT_DIR/rules" ]; then
    if [ "$FORCE" = true ] || [ ! -d "$TARGET/.cursor/rules" ]; then
        rm -rf "$TARGET/.cursor/rules"
        cp -r "$SCRIPT_DIR/rules" "$TARGET/.cursor/rules"
        echo "  Created: .cursor/rules/ (isolation rules, visual maps, etc.)"
    else
        echo "  Skipped: .cursor/rules/ (already exists)"
    fi
else
    echo "  Skipped: .cursor/rules/ (source not found)"
fi

# Copy .cursorindexingignore
if [ -f "$REPO_DIR/.cursorindexingignore" ]; then
    if [ "$FORCE" = true ] || [ ! -f "$TARGET/.cursorindexingignore" ]; then
        cp "$REPO_DIR/.cursorindexingignore" "$TARGET/.cursorindexingignore"
        echo "  Created: .cursorindexingignore"
    else
        echo "  Skipped: .cursorindexingignore (already exists)"
    fi
else
    echo "  Skipped: .cursorindexingignore (source not found)"
fi

echo ""
echo "Memory Bank (Cursor IDE) initialized."
echo ""
echo "Usage: Open your project in Cursor IDE. Either run stages manually:"
echo "  /van → /plan → /creative → /build → /reflect → /archive"
echo "...or run the whole pipeline automatically (per-stage models via .cursor/agents/):"
echo "  /orchestrate <task description>"
echo ""
echo "The system routes complexity automatically:"
echo "  Level 1 (bug fix):     VAN → BUILD → REFLECT"
echo "  Level 2 (enhancement): VAN → PLAN → BUILD → REFLECT"
echo "  Level 3 (feature):     VAN → PLAN → CREATIVE → BUILD → REFLECT"
echo "  Level 4 (system):      Full pipeline + ARCHIVE"
