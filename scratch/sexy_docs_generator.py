#!/usr/bin/env python3
import os
import sys
import json
import urllib.request
import urllib.parse
import urllib.error
import subprocess

def run_git_cmd(args):
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode != 0:
        print(f"❌ Git error running command: {' '.join(args)}")
        print(result.stderr)
        return False
    return True

def get_vertex_credentials():
    adc_path = os.path.expanduser("~/.config/gcloud/application_default_credentials.json")
    if not os.path.exists(adc_path):
        return None
        
    print(f"   🔎 Located Application Default Credentials file at: {adc_path}")
    try:
        with open(adc_path, "r") as f:
            creds = json.load(f)
            
        project_id = creds.get("quota_project_id") or creds.get("project_id")
        refresh_token = creds.get("refresh_token")
        client_id = creds.get("client_id")
        client_secret = creds.get("client_secret")
        
        if not refresh_token or not client_id or not client_secret:
            return None
            
        # Exchange refresh token for a fresh access token
        url = "https://oauth2.googleapis.com/token"
        payload = {
            "client_id": client_id,
            "client_secret": client_secret,
            "refresh_token": refresh_token,
            "grant_type": "refresh_token"
        }
        
        req = urllib.request.Request(
            url,
            data=urllib.parse.urlencode(payload).encode("utf-8"),
            headers={"Content-Type": "application/x-www-form-urlencoded"}
        )
        
        with urllib.request.urlopen(req) as response:
            res_data = json.loads(response.read().decode("utf-8"))
            access_token = res_data.get("access_token")
            if access_token and project_id:
                print(f"   🔑 Authenticated via GCP Project: '{project_id}' using Application Default Credentials (direct OAuth exchange).")
                return (project_id, access_token)
    except Exception as e:
        print(f"   ⚠️  Failed to retrieve access token from ADC file: {e}")
        
    return None

def query_gemini_api(api_key, model, prompt, vertex_creds=None):
    if vertex_creds:
        project_id, access_token = vertex_creds
        # Vertex AI endpoint
        url = f"https://us-central1-aiplatform.googleapis.com/v1/projects/{project_id}/locations/us-central1/publishers/google/models/{model}:generateContent"
        headers = {
            "Content-Type": "application/json",
            "Authorization": f"Bearer {access_token}"
        }
    else:
        # Gemini Developer API
        url = f"https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={api_key}"
        headers = {
            "Content-Type": "application/json"
        }
        
    payload = {
        "contents": [
            {
                "role": "user",
                "parts": [
                    {"text": prompt}
                ]
            }
        ],
        "generationConfig": {
            "temperature": 0.2,
            "maxOutputTokens": 8192
        }
    }
    
    req = urllib.request.Request(
        url,
        data=json.dumps(payload).encode("utf-8"),
        headers=headers
    )
    
    try:
        with urllib.request.urlopen(req) as response:
            res_data = json.loads(response.read().decode("utf-8"))
            parts = res_data["candidates"][0]["content"]["parts"]
            text = "".join(part.get("text", "") for part in parts)
            
            # Clean wrapping markdown indicators if returned by the model
            text = text.strip()
            if text.startswith("```markdown"):
                text = text[11:].strip()
            elif text.startswith("```"):
                text = text[3:].strip()
            if text.endswith("```"):
                text = text[:-3].strip()
            return text
            
    except urllib.error.HTTPError as e:
        print(f"❌ API Request failed: {e.code} {e.reason}")
        print(e.read().decode("utf-8"))
        sys.exit(1)
    except Exception as e:
        print(f"❌ Unexpected API error: {e}")
        sys.exit(1)

def main():
    print("🌌 ======================================================= 🌌")
    print("🚀  Korg Virtual 'NotebookLM' Documentation Suite Generator")
    print("🌌 ======================================================= 🌌\n")

    # 1. Check for Authentication Method
    api_key = os.environ.get("GEMINI_API_KEY")
    vertex_creds = None
    
    print("🔒 Authenticating session...")
    
    if not api_key:
        # Try finding Google Application Default Credentials (ADC)
        vertex_creds = get_vertex_credentials()
        
    if not api_key and not vertex_creds:
        print("🔑 GEMINI_API_KEY and active Google Cloud ADC session not found.")
        choice = input("👉 Choose authentication: [1] Enter Gemini API Key, [2] Exit and run gcloud ADC setup: ").strip()
        if choice == "1":
            api_key = input("👉 Paste your Google Gemini API Key: ").strip()
            if not api_key:
                print("❌ Error: A valid Gemini API Key is required.")
                sys.exit(1)
        else:
            print("👋 Exited. Please authenticate your Google Cloud CLI or set GEMINI_API_KEY.")
            sys.exit(0)

    # 2. Build Virtual Notebook Ingest Context
    print("\n📚 [Step 1/4] Ingesting codebase files into the virtual Notebook...")
    
    source_files = [
        ("Cargo.toml", "Project dependencies and metadata"),
        ("DOCS.md", "Master Documentation Specifications and Cinematic Q&As"),
        ("src/web.rs", "Axum Web Cockpit showing SSE streams, telemetry, and actions drawer"),
        ("src/leader.rs", "Orchestrator Kernel managing campaigns, sandbox worktrees, and steering forks"),
        ("src/vision_policy.rs", "Vision Policy Engine OCR security scans and fail-secure redaction"),
        ("src/provenance.rs", "Provenance cryptographic ledger, signatures, and JCS parent-hash linking"),
        ("src/evaluator.rs", "Zero-Trust Scorer analyzing tasks using Candle-BERT semantic similarity")
    ]
    
    context = ""
    for filepath, desc in source_files:
        if os.path.exists(filepath):
            print(f"   📥 Ingesting: {filepath} ({desc})")
            with open(filepath, "r", errors="ignore") as f:
                content = f.read()
            context += f"\n\n=== FILE SOURCE: {filepath} ({desc}) ===\n{content}\n"
        else:
            print(f"   ⚠️  Warning: {filepath} not found. Skipping context...")

    # Choose model
    # On Vertex AI, the model is referred to as gemini-2.5-pro, same as Gemini Developer API.
    model_name = "gemini-2.5-pro"
    print(f"\n🧠 Using high-assurance model: {model_name}")

    # 3. Generate README.md
    print("\n✍️  [Step 2/4] Synthesizing ultra-sexy, Grok-inspired README.md...")
    readme_prompt = f"""
You are the principal systems architect and lead designer at Google DeepMind and xAI. 
Your task is to take the Korg codebase context below and synthesize an absolute masterpiece of a GitHub `README.md`.
It must be stunning, visceral, highly descriptive, and target the top 0.1% of systems engineers and open-source developers.

CRITICAL FORMATTING RULES:
1. Do NOT write any conversational intro or outro text (e.g., "Here is your README:"). Start directly with `# korg — Autonomous Software Engineering Runtime`.
2. Output ONLY the raw markdown content.
3. Incorporate beautiful modern badges, comparison tables, and quickstarts.
4. Include a visual, high-tempo ASCII dashboard showing the Zero-Overlap inline layout (Amber Security Gate, Emerald Consensus, Cyan Steering Fork) and sparkline charts.
5. Define the mathematical formulas for the Core Theoretical Pillars (BERT cosine contract negotiation, RFC 8785 canonical Merkle-DAG chain serialization, and the fail-secure visual OCR firewall) using clean LaTeX equations.
6. Provide clear, copy-pasteable cargo/git quickstart terminal lines.

Here is the virtual Notebook context for Korg:
{context}
"""
    readme_content = query_gemini_api(api_key, model_name, readme_prompt, vertex_creds)
    with open("README.md", "w") as f:
        f.write(readme_content)
    print("   💾 Overwrote README.md successfully.")

    # 4. Generate ARCHITECTURE.md
    print("\n✍️  [Step 3/4] Synthesizing comprehensive, deep-dive ARCHITECTURE.md...")
    arch_prompt = f"""
You are the lead security auditor and principal systems programmer for Korg.
Generate an incredibly rich, master-class technical reference document for `ARCHITECTURE.md` explaining Korg's internals.
Explain exactly how the rust backend maintains absolute memory safety and isolated execution.

CRITICAL FORMATTING RULES:
1. Start directly with the title `# Korg System Architecture & Internals`. Do NOT write any conversational preambles.
2. Output ONLY the raw markdown content.
3. Detail the decoupled, transactional CRDT Blackboard (`src/blackboard.rs`) and how concurrency is structured.
4. Deep-dive into the 4-Persona Adversarial Swarm collaboration topology (Orchestrator Captain, Auditor Harper, Builder Benjamin, Synthesizer Lucas).
5. Graph the state transition sequence using a beautiful, clean mermaid diagram.
6. Detail the cryptographic provenance chain (`src/provenance.rs`), explaining parent-hash linking, JCS canonicalization, and playhead timeline scrubbing.
7. Detail the OCR Pixel Redaction and Fail-Secure visual firewall loops inside `src/vision_policy.rs`.

Here is the virtual Notebook context for Korg:
{context}
"""
    arch_content = query_gemini_api(api_key, model_name, arch_prompt, vertex_creds)
    with open("ARCHITECTURE.md", "w") as f:
        f.write(arch_content)
    print("   💾 Overwrote ARCHITECTURE.md successfully.")

    # 5. Generate USER_GUIDE.md
    print("\n✍️  [Step 4/4] Synthesizing interactive, step-by-step USER_GUIDE.md...")
    user_prompt = f"""
You are the head of developer relations and technical training at Korg.
Write an exceptionally friendly, clear, and comprehensive `USER_GUIDE.md` detailing how to use Korg's cockpit.
Showcase Korg's visual terminal experience and zero-overlap layout.

CRITICAL FORMATTING RULES:
1. Start directly with the title `# Korg User Guide & Cockpit Manual`. Do NOT write any conversational preambles.
2. Output ONLY the raw markdown content.
3. Provide a step-by-step tutorial walkthrough of running a campaign from prompt to validation.
4. Walk through the interactive cockpit TUI and Axum SSE console, describing the 6-pane grid.
5. Create an elegant, easy-to-read keyboard shortcut table (`q` for exit, `Arrow Keys` for time-travel scrubbing, `F` for Playhead steering forks, `Y`/`N` for manual override bypasses).
6. Give actionable scenarios:
   - Scenario A: Benjamin triggers a zero-trust policy block when running restricted curls, showing the flashing security card.
   - Scenario B: Operator scrubs playhead back to tx_03 and triggers an 'F' fork to rewrite a db layer with memory-mapped vectors.

Here is the virtual Notebook context for Korg:
{context}
"""
    user_content = query_gemini_api(api_key, model_name, user_prompt, vertex_creds)
    with open("USER_GUIDE.md", "w") as f:
        f.write(user_content)
    print("   💾 Overwrote USER_GUIDE.md successfully.")

    # 6. Commit and Push to Git
    print("\n🚀 [Git Automation] Staging, committing, and pushing documentation to GitHub...")
    if run_git_cmd(["git", "add", "README.md", "ARCHITECTURE.md", "USER_GUIDE.md"]):
        if run_git_cmd(["git", "commit", "-m", "docs: update complete documentation suite via virtual Gemini Notebook"]):
            if run_git_cmd(["git", "push", "origin", "main"]):
                print("\n🏆 SUCCESS: All documentation is fully synchronized, committed, and pushed LIVE!")
                print("🌌 Thank you for using Korg's Virtual NotebookLM Swarm! 🌌\n")
                sys.exit(0)
                
    print("\n❌ Git Error: Failed to automatically push changes to the remote repository.")
    print("👉 Please run 'git push origin main' manually in your terminal.")

if __name__ == "__main__":
    main()
