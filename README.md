# Rzn:Browser Bridge - Native Browser Control System

👋 Welcome to Rzn:Browser Bridge! This system demonstrates an architecture for browser automation using a Chrome extension that communicates with local Rust applications. By leveraging Chrome's Native Messaging and inter-process communication, we've created a flexible and maintainable solution for browser control tasks.

## Using This Template

This repository is set up as a GitHub template. To use it:

1. Click the "Use this template" button at the top of the repository page
2. Choose a name for your new repository
3. Select the owner (your account or an organization)
4. Choose public or private visibility
5. Click "Create repository from template"

After creating your repository, you'll need to:
- Update the project name in the README and LICENSE files
- Update the extension ID in the manifest files (see Setup Instructions)
- Customize the application and extension logic for your specific needs
- Review the extension's package.json and adjust dependencies if needed
- Consider updating the broker name in the manifest to match your project name

## Table of Contents

* [Overview](#overview)
* [Why This Architecture?](#why-this-architecture)
* [How It Works](#how-it-works)
* [Project Structure](#project-structure)
* [Setup Instructions](#setup-instructions)
* [Trying It Out](#trying-it-out)
* [Design Considerations](#design-considerations)
* [Future Enhancements](#future-enhancements)

## Overview

This project connects three main components:

1. **Chrome Extension**: Runs in the browser and initiates actions
2. **Broker (`rzn_broker`)**: Handles Native Messaging with Chrome and relays messages
3. **Main Application (`example_app`)**: Processes requests and implements core functionality

Together, these components provide a foundation for browser automation, web scraping, or any task that requires communication between a browser extension and local applications.

## Why This Architecture?

You might wonder why we've chosen a three-part architecture instead of a simpler design. Here's our rationale:

### The Broker Advantage

* **Separation of Concerns**: The broker focuses solely on communication protocol handling, while the main app focuses on business logic
* **Process Independence**: The main application can be developed, run, and restarted separately from the browser connection
* **Fault Tolerance**: If the main app crashes, the broker can potentially restart it or handle errors gracefully
* **Protocol Isolation**: The main app doesn't need to implement the Native Messaging protocol

### Technical Foundation

* **Native Messaging**: Chrome's standard for extension-to-native-app communication
* **Inter-Process Communication (IPC)**: Efficient local socket/pipe communication between the broker and main app
* **Asynchronous Processing**: Both Rust applications use Tokio for responsive, non-blocking I/O handling

## How It Works

Here's a step-by-step walkthrough of a simple "ping" request:

1. **Extension**: Initiates a request (e.g., `sendSimplePing()` in the console)
2. **Extension → Chrome**: Sends a JSON message through Native Messaging
3. **Chrome → Broker**: Chrome launches the broker if needed and writes the message to its stdin
4. **Broker**: Reads the message and forwards it through the IPC socket
5. **Main App**: Receives the message, processes it, and generates a response
6. **Main App → Broker**: Sends the response back through the IPC socket
7. **Broker → Chrome**: Writes the response to stdout
8. **Chrome → Extension**: Delivers the response to the extension's message listener
9. **Extension**: Processes and displays the response

The message flow looks like this:

```
+-------------------+      stdin/stdout      +-----------------+      IPC Socket      +-------------------+
| Chrome Extension  | <--------------------> |  rzn_broker     | <----------------> |  example_app      |
+-------------------+  (Native Messaging)    +-----------------+  (interprocess)    +-------------------+
```

## Project Structure

```
.
├── com.yourcompany.projectagentis.broker.json  # Native Messaging Host Manifest
├── extension/                     # Chrome Extension files
│   ├── src/
│   │   └── background.js         # Extension logic
│   └── manifest.json             # Extension Manifest
├── example_app/                   # Main Application (Rust)
│   ├── src/
│   │   └── main.rs               # Main app logic
│   └── Cargo.toml
├── rzn_broker/                    # Broker Application (Rust)
│   ├── src/
│   │   └── main.rs               # Broker logic
│   └── Cargo.toml
├── setup.sh                       # Build and installation script
└── Cargo.toml                     # Workspace Cargo file
```

## Setup Instructions

### Prerequisites

* **Rust & Cargo**: [Install the Rust toolchain](https://www.rust-lang.org/tools/install)
* **Node.js & npm**: Required to build the Chrome extension
* **Google Chrome**: Or any Chromium-based browser supporting Native Messaging

### Installation Steps

1. **Clone the Repository**
   ```bash
   git clone <your-repo-url>
   cd <your-repo-directory>
   ```

2. **Run the Setup Script**
   ```bash
   chmod +x setup.sh
   ./setup.sh
   ```
   This script:
   * Installs Node.js dependencies for the extension
   * Builds the Chrome extension
   * Builds the Rust applications in release mode
   * Determines the correct Native Messaging Host directory for your OS
   * Creates and installs the manifest file with the correct path to the broker executable

3. **Load the Extension**
   * Go to `chrome://extensions` in Chrome
   * Enable "Developer mode" (toggle in the top-right)
   * Click "Load unpacked"
   * Select the `extension/dist` directory (created during setup)
   * Note your extension's ID shown on the card

4. **Update the Native Messaging Host Manifest**
   * Find your extension's ID in `chrome://extensions`
   * Edit the manifest file at the location shown in the setup script output
   * Replace `"REPLACE_WITH_YOUR_EXTENSION_ID"` with your actual extension ID in the `allowed_origins` field

5. **Verify Extension Installation**
   * The extension icon should appear in your browser toolbar
   * If it doesn't appear automatically, you may need to pin it from the extensions menu

## Trying It Out

1. **Start the Example App**
   ```bash
   RUST_LOG=info ./target/release/example_app
   ```
   You should see a message indicating it's listening for connections

2. **Test the Connection**
   * Go to `chrome://extensions`
   * Find the "Rzn:Browser Bridge" extension
   * Click the link to inspect views/background page
   * In the console, type `sendSimplePing()` and press Enter

3. **Observe the Results**
   * **Extension Console**: Should show the sent ping and received pong
   * **Example App Terminal**: Should show logs about receiving the ping and sending the response

## Design Considerations

* **Message Format**: JSON provides human-readability and cross-language compatibility
* **Message Framing**: Each message is prefixed with a 4-byte length to ensure proper message boundaries
* **Error Handling**: Basic logging with potential for more sophisticated error recovery
* **Cross-Platform**: The `interprocess` crate handles platform-specific IPC mechanisms
* **Security**: Native Messaging provides extension isolation, with Chrome managing permissions

### Known Limitations

* Message structs are currently duplicated in both Rust applications
* Error handling is minimal (primarily logging)
* The broker does not currently attempt to launch the main app if it's not running

## Future Enhancements

* **Shared Types Library**: Move message definitions to a dedicated crate
* **Real Browser Automation**: Implement actual control logic using `headless_chrome` or Playwright
* **Robust Error Handling**: Add retry logic and better error reporting
* **Task Queue**: Support multiple concurrent automation tasks
* **Auto-Launch**: Allow the broker to start the main app if needed
* **Configuration**: Make socket names and paths configurable
* **Packaging**: Create installer scripts for easier distribution
* **Security Enhancements**: Add message validation and permission controls

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

