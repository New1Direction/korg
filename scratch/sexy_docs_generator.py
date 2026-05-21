#!/usr/bin/env python3
import os
import sys
import json
import urllib.request
import urllib.error
import subprocess

def run_git_cmd(args):
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode != 0:
        print(f"❌ Git error running command: {' '.join(args)}")
        print(result.stderr)
        return False
    return True

def main():
    print("🌌 Starting Korg Ultra-Sexy Documentation Generator...")

    # 1. Retrieve Gemini API Key
    api_key = os.environ.get("GEMINI_API_KEY")
    if not api_key:
        print("🔑 GEMINI_API_KEY environment variable not found.")
        api_key = input("👉 Please paste your Google Gemini API Key: ").strip()
    
    if not api_key:
        print("❌ API Key is required to run the generator.")
        sys.exit(1)

    # 2. Read Source Files for Context
    print("📖 Ingesting codebase and design documentation...")
    context = ""
    
    source_files = [
        ("DOCS.md", "Master Documentation Specifications"),
        ("src/web.rs", "Axum Web Cockpit and Zero-Overlap UI Implementation (HTML/CSS/JS)"),
        ("scratch/sync_notebooklm_py.py", "NotebookLM Py-Automation Script")
    ]

    for filepath, desc in source_files:
        if os.path.exists(filepath):
            with open(filepath, "r") as f:
                content = f.read()
            context += f"\n\n=== SOURCE: {filepath} ({desc}) ===\n{content}\n"
        else:
            print(f"⚠️  Context source {filepath} not found. Skipping...")

    # 3. Formulate Prompt for Gemini
    prompt = f"""
You are the lead technical writer and principal designer at Google DeepMind and xAI. 
Your task is to take the provided Korg codebase context and write an absolute masterpiece of a GitHub `README.md`. 
It must be ultra-sexy, cinematic, and visually stunning, targeting the top 0.1% of systems engineers and open-source developers.

CRITICAL REQUIREMENTS:
1. DO NOT include any conversational intro or outro text (e.g. "Here is your README:"). Output ONLY the raw markdown content.
2. Use dynamic modern badges and styling.
3. Design a beautiful, premium visual ASCII dashboard layout diagram showing Korg's active Left Column grid, live sparkline charts, and the newly implemented zero-overlap inline actions panel (Amber Security Gate, Emerald Consensus, Cyan Steering Fork).
4. Outline the Core Theoretical Pillars (Adversarial Consensus cosine formulas, Merkle-DAG chain serialization equations, and the fail-secure multiline OCR Vision Policy Engine) using elegant LaTeX/Math formulations.
5. Provide a sleek comparison table highlighting why Korg is lightyears ahead of traditional, passive, and insecure AI agents (like Aider or AutoGen).
6. Detail the complete quickstart guide, including the local AXUM cockpit, git staging loops, and our new automated Playwright screen recorder.

Here is the Korg system context:
{context}
"""

    # 4. Make API Call to Gemini
    print("🧠 Consulting Gemini to synthesize ultra-premium documentation...")
    url = f"https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-pro:generateContent?key={api_key}"
    
    payload = {
        "contents": [
            {
                "parts": [
                    {"text": prompt}
                ]
            }
        ]
    }
    
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers={"Content-Type": "application/json"}
    )

    try:
        with urllib.request.urlopen(req) as response:
            res_data = json.loads(response.read().decode("utf-8"))
            generated_markdown = res_data["candidates"][0]["content"]["parts"][0]["text"]
            
            # Clean markdown code block wraps if the model returned them
            if generated_markdown.startswith("```markdown"):
                generated_markdown = generated_markdown[11:]
            elif generated_markdown.startswith("```"):
                generated_markdown = generated_markdown[3:]
            if generated_markdown.endswith("```"):
                generated_markdown = generated_markdown[:-3]
            generated_markdown = generated_markdown.strip()

    except urllib.error.HTTPError as e:
        print(f"❌ API Request failed: {e.code} {e.reason}")
        print(e.read().decode("utf-8"))
        sys.exit(1)
    except Exception as e:
        print(f"❌ Unexpected API error: {e}")
        sys.exit(1)

    # 5. Overwrite README.md
    print("📝 Overwriting README.md with synthesized masterpiece...")
    with open("README.md", "w") as f:
        f.write(generated_markdown)
    print("🎉 README.md written successfully!")

    # 6. Stage and Push to Git
    print("🚀 Automatically committing and pushing changes to GitHub...")
    if run_git_cmd(["git", "add", "README.md"]):
        if run_git_cmd(["git", "commit", "-m", "docs: update master README using automated Gemini generator"]):
            if run_git_cmd(["git", "push", "origin", "main"]):
                print("🏆 Sexy Git Documentation is now LIVE on GitHub!")
                sys.exit(0)
    
    print("❌ Failed to push updates to Git.")

if __name__ == "__main__":
    main()
