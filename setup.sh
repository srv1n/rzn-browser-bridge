#!/bin/bash
set -e

echo "===== Rzn:Browser Bridge Setup ====="
echo "Building Rust components and Chrome extension..."

# Build Rust applications
echo "Building Rust applications..."
cargo build --release

# Install Node.js dependencies and build the extension
echo "Building Chrome extension..."
cd extension
if [ ! -d "node_modules" ]; then
  echo "Installing extension dependencies..."
  npm install
else
  echo "Node modules already installed, skipping npm install."
fi

# Build the extension
echo "Running npm build for extension..."
npm run build
cd ..

# Determine correct Native Messaging Host directory based on OS
echo "Determining Native Messaging Host directory for your OS..."
MANIFEST_NAME="com.yourcompany.projectagentis.broker.json"
HOST_DIR=""

if [[ "$OSTYPE" == "linux-gnu"* ]]; then
  # Linux
  HOST_DIR="$HOME/.config/google-chrome/NativeMessagingHosts"
  # Create directory if it doesn't exist
  mkdir -p "$HOST_DIR"
elif [[ "$OSTYPE" == "darwin"* ]]; then
  # macOS
  HOST_DIR="$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts"
  # Create directory if it doesn't exist
  mkdir -p "$HOST_DIR"
elif [[ "$OSTYPE" == "msys"* ]] || [[ "$OSTYPE" == "win32" ]]; then
  # Windows via Git Bash or similar
  # Adjust if needed for proper Windows path handling
  HOST_DIR="$APPDATA/Google/Chrome/NativeMessagingHosts"
  # Create directory if it doesn't exist
  mkdir -p "$HOST_DIR"
else
  echo "Unsupported OS: $OSTYPE"
  echo "Please manually install the Native Messaging Host manifest."
  exit 1
fi

# Get absolute path to broker executable
BROKER_PATH="$(pwd)/target/release/rzn_broker"
if [[ "$OSTYPE" == "msys"* ]] || [[ "$OSTYPE" == "win32" ]]; then
  # Windows needs .exe extension
  BROKER_PATH="${BROKER_PATH}.exe"
fi

# Create and install the manifest
echo "Creating Native Messaging Host manifest at $HOST_DIR/$MANIFEST_NAME..."
cat > "$HOST_DIR/$MANIFEST_NAME" << EOL
{
  "name": "com.yourcompany.projectagentis.broker",
  "description": "Rzn:Browser Bridge Broker",
  "path": "$BROKER_PATH",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://REPLACE_WITH_YOUR_EXTENSION_ID/"
  ]
}
EOL

echo "===== Setup Complete ====="
echo ""
echo "IMPORTANT: Next steps to complete setup:"
echo "1. Go to chrome://extensions/ in Chrome"
echo "2. Enable 'Developer mode' (toggle in top-right)"
echo "3. Click 'Load unpacked' and select the extension/dist directory"
echo "4. Note your extension ID from the card"
echo "5. Edit $HOST_DIR/$MANIFEST_NAME"
echo "6. Replace 'REPLACE_WITH_YOUR_EXTENSION_ID' with your actual extension ID"
echo ""
echo "To test the system:"
echo "1. Start the example app: RUST_LOG=info ./target/release/example_app"
echo "2. In Chrome, find the extension and click to inspect background page"
echo "3. In the console, type sendSimplePing()"
echo "" 