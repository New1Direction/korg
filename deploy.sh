#!/usr/bin/env bash
# korg one-command production deployer

set -euo pipefail

APP_DIR="/home/clubpenguin/korg-deploy"
REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=== ⚡ STARTING KORG PRODUCTION DEPLOYMENT ⚡ ==="

# 1. Compile release binary
echo "--> Compiling release binary in $REPO_DIR..."
cd "$REPO_DIR"
cargo build --release

# 2. Build directories
echo "--> Rebuilding clean deploy target..."
mkdir -p "$APP_DIR"

# 3. Assemble release bundle
echo "--> Copying binary and configurations..."
cp target/release/korg "$APP_DIR/korg"
cp korg.toml "$APP_DIR/korg.toml"

# 4. Check/Install service configuration
if [ ! -f /etc/systemd/system/korg.service ]; then
    echo "--> Systemd service not found. Creating korg.service..."
    sudo tee /etc/systemd/system/korg.service > /dev/null <<EOF
[Unit]
Description=Korg Autonomous Software Engineering Runtime
After=network.target

[Service]
Type=simple
User=clubpenguin
WorkingDirectory=$APP_DIR
ExecStart=$APP_DIR/korg campaign --web
Restart=always
RestartSec=5
Environment="RUST_LOG=info"
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF
    sudo systemctl daemon-reload
    sudo systemctl enable korg
fi

# 5. Reload and Restart Servers
echo "--> Restarting Caddy reverse-proxy and Korg..."
sudo systemctl restart caddy korg

echo "=== 🚀 KORG IS LIVE AT https://yvaehkorg.lol ✓ ==="
