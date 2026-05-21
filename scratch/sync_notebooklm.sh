#!/bin/bash

# exit on error
set -e

echo "🌌 Starting Korg -> NotebookLM Synchronization Automation..."

# 1. Check for nlm command-line tool
if ! command -v nlm &> /dev/null; then
    echo "⚠️  NotebookLM CLI ('nlm') not found in PATH."
    echo "📦 Attempting to install notebooklm-mcp-cli via 'uv'..."
    
    if command -v uv &> /dev/null; then
        uv tool install notebooklm-mcp-cli
    else
        echo "📦 'uv' not found. Attempting install via pip..."
        pip install notebooklm-mcp-cli
    fi
fi

# 2. Check Authentication
echo "🔍 Diagnosing NotebookLM authentication status..."
if ! nlm doctor &> /dev/null; then
    echo "🔑 Authentication missing or expired."
    echo "👉 Please run 'nlm login' on your terminal to connect your Google account browser cookies."
    echo "   (This script will pause to let you authenticate)"
    nlm login
fi

# 3. Create or Locate 'korg' Notebook
echo "🔍 Syncing 'Korg Swarm Engine' notebook..."
# Check if korg alias is already registered
if ! nlm notebook list | grep -q "korg"; then
    echo "🆕 Creating new Notebook: 'Korg Swarm Engine'..."
    NOTEBOOK_ID=$(nlm notebook create "Korg Swarm Engine" | grep -Eo '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}' | head -n 1)
    
    if [ -n "$NOTEBOOK_ID" ]; then
        echo "🏷️  Setting shortcut alias 'korg' to ID: $NOTEBOOK_ID"
        nlm alias set korg "$NOTEBOOK_ID"
    else
        echo "❌ Failed to retrieve Notebook ID. Creating via default list fallback..."
    fi
else
    echo "✅ 'Korg Swarm Engine' notebook is already registered and aliased."
fi

# 4. Synchronize Source Files
echo "⚡ Uploading master documentation 'DOCS.md'..."
if [ -f "DOCS.md" ]; then
    nlm source add korg --file DOCS.md
    echo "🎉 Successfully synchronized 'DOCS.md' to Korg NotebookLM!"
else
    echo "❌ DOCS.md not found in the current directory."
    exit 1
fi

echo "🚀 Synchronization complete! Head over to https://notebooklm.google/ to generate your high-tempo Audio Overview podcast!"
