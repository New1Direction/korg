#!/usr/bin/env python3
import subprocess
import sys
import json
import re

def run_cmd(args):
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode != 0:
        print(f"❌ Error running command: {' '.join(args)}")
        print(result.stderr)
        return None
    return result.stdout.strip()

def get_notebooks():
    output = run_cmd(["notebooklm", "list"])
    if not output:
        return []
    
    notebooks = []
    # Parse list output
    # Typical format: ID | Title
    lines = output.split('\n')
    for line in lines:
        if '|' in line:
            parts = [p.strip() for p in line.split('|')]
            if len(parts) >= 2:
                # First column is usually ID, second is Title
                notebooks.append({"id": parts[0], "title": parts[1]})
    return notebooks

def main():
    print("🌌 Starting Korg -> NotebookLM Py-Automation...")
    
    # Check if logged in
    status = run_cmd(["notebooklm", "status"])
    if not status or "Not logged in" in status:
        print("🔑 You need to authenticate with Google first.")
        print("👉 Please open your terminal and run:")
        print("   notebooklm login")
        print("\nThis will open a browser window on your Mac screen. Log in, press ENTER, and then run this script again!")
        sys.exit(1)

    notebooks = get_notebooks()
    korg_nb = None
    for nb in notebooks:
        if "Korg Swarm Engine" in nb["title"]:
            korg_nb = nb
            break

    if korg_nb:
        print(f"✅ Found existing notebook: 'Korg Swarm Engine' (ID: {korg_nb['id']})")
    else:
        print("🆕 Creating new notebook: 'Korg Swarm Engine'...")
        create_output = run_cmd(["notebooklm", "create", "Korg Swarm Engine"])
        if not create_output:
            print("❌ Failed to create notebook.")
            sys.exit(1)
        
        # Parse ID from creation output
        match = re.search(r'([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})', create_output)
        if match:
            korg_nb = {"id": match.group(1), "title": "Korg Swarm Engine"}
            print(f"🎉 Created notebook successfully (ID: {korg_nb['id']})")
        else:
            # Fallback: re-fetch notebooks
            notebooks = get_notebooks()
            for nb in notebooks:
                if "Korg Swarm Engine" in nb["title"]:
                    korg_nb = nb
                    break

    if not korg_nb:
        print("❌ Could not resolve 'Korg Swarm Engine' notebook ID.")
        sys.exit(1)

    # Set context to the notebook
    print(f"🔌 Setting active notebook context to: {korg_nb['id']}...")
    run_cmd(["notebooklm", "use", korg_nb["id"]])

    # Add source DOCS.md
    print("⚡ Uploading 'DOCS.md' to the notebook sources...")
    add_output = run_cmd(["notebooklm", "source", "add", "DOCS.md"])
    if add_output:
        print("🎉 Successfully uploaded 'DOCS.md'!")
        print(add_output)
    
    print("\n--- NEXT STEPS ---")
    print("🎧 To generate your professional Audio Overview podcast, run:")
    print("   notebooklm generate audio")
    print("\n📩 Once generated, you can download the MP3 file directly with:")
    print("   notebooklm download audio")

if __name__ == "__main__":
    main()
