#!/bin/bash
set -e

echo "Building Rust broker and example app..."
cargo build --release

echo "Installing native messaging host manifest..."

# Get the OS type
if [[ "$OSTYPE" == "darwin"* ]]; then
    # macOS
    MANIFEST_DIR="$HOME/Library/Application Support/Google/Chrome/NativeMessagingHosts"
elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    # Linux
    MANIFEST_DIR="$HOME/.config/google-chrome/NativeMessagingHosts"
else
    echo "Unsupported OS: $OSTYPE"
    echo "Please manually install the manifest file according to the README"
    exit 1
fi

# Create the target directory if it doesn't exist
mkdir -p "$MANIFEST_DIR"

# Use the new manifest file
MANIFEST_NAME="com.yourcompany.projectagentis.broker.json"

# Get the absolute path to the broker executable
EXECUTABLE_PATH=$(realpath target/release/rzn_broker)

# Create a temporary file with the correct path
TMP_MANIFEST=$(mktemp)
cat "$MANIFEST_NAME" | sed "s|\"path\": \".*\"|\"path\": \"$EXECUTABLE_PATH\"|g" > "$TMP_MANIFEST"

# Copy the updated manifest to Chrome's directory
cp "$TMP_MANIFEST" "$MANIFEST_DIR/$MANIFEST_NAME"
rm "$TMP_MANIFEST"

echo "✅ Setup completed successfully!"
echo "Native messaging host installed at: $MANIFEST_DIR/$MANIFEST_NAME"
echo "Make sure the Chrome extension is using the name 'com.yourcompany.projectagentis.broker' when connecting"
echo ""
echo "To test the system:"
echo "1. Start the example app: ./target/release/example_app"
echo "2. Start Chrome with the extension loaded"
echo "3. The broker will automatically start when the extension connects" 