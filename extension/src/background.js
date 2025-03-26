// Native messaging host name (this should match what you'll set in the host manifest)
const HOST_NAME = "com.yourcompany.projectagentis.broker"; // Ensure this matches the broker's expected ID in its manifest

// Connect to the native messaging host
let port = null;
let isTestRunning = false; // Add this flag to prevent multiple simultaneous tests
let initialConnectionAttempted = false; // Track if we've already tried to connect
let reconnectAttempts = 0; // Count reconnection attempts

// --- Function to send a simple test message ---
function sendSimplePing() {
    if (!port) {
        console.error("Cannot send ping: Native host not connected.");
        // Optionally try to reconnect
        // connectToNative();
        return;
    }
    const pingMessage = {
        action: "ping",
        task_id: `ping-${Date.now()}`, // Include a unique ID
        data: { message: "Hello from extension!" }
    };
    try {
        console.log("Sending ping message:", pingMessage);
        port.postMessage(pingMessage);
    } catch (error) {
        console.error("Error sending ping message:", error);
        // Handle potential disconnection
        if (error.message.includes("disconnected")) {
            port = null;
            initialConnectionAttempted = false;
            // Maybe attempt reconnect after a delay
        }
    }
}
// --- End of simple test message function ---

function connectToNative() {
    // Prevent repeated connection attempts during startup
    if (initialConnectionAttempted && port) {
        console.log("Already connected, skipping reconnection");
        return;
    }
    
    // Limit reconnection attempts to prevent infinite loops
    if (reconnectAttempts > 5) {
        console.log("Too many reconnection attempts, waiting for manual action");
        setTimeout(() => { reconnectAttempts = 0; }, 60000); // Reset after 1 minute
        return;
    }
    if (port) {
        console.log("Already connected or attempting connection.");
        return; // Avoid multiple concurrent connection attempts
    }

    initialConnectionAttempted = true;
    reconnectAttempts++;
    
    try {
        console.log("Connecting to native host:", HOST_NAME);
        port = chrome.runtime.connectNative(HOST_NAME);
        reconnectAttempts = 0; // Reset attempts on successful connection start

        port.onMessage.addListener((message) => {
            // --- Updated Message Handling ---
            console.log("<<< Received message from native host:", message);

            if (message.action === "pong") {
                console.log("Received PONG response:", message);
                // Handle the pong response (e.g., update UI, confirm connection)
            } else if (message.action === "perform_task") {
                console.log("Received 'perform_task' action with task_id:", message.task_id);
                handleTask(message); // Pass to the existing task handler
            } else if (message.action === "task_result") {
                // This case should ideally NOT happen if the broker is just relaying
                // The example_app sends task_result, broker relays, extension receives.
                // Let's log it if it does come through for debugging.
                 console.log("Received 'task_result' (likely relayed from example_app):", message);
            }
             else {
                console.warn("Received unknown message action:", message.action, message);
            }
            // --- End Updated Message Handling ---
        });

        port.onDisconnect.addListener(() => {
            const lastError = chrome.runtime.lastError;
            console.error("Native host disconnected.", lastError ? lastError.message : "(No error message)");
            port = null;
            initialConnectionAttempted = false; // Allow future connection attempts

            // Optional: Schedule a delayed reconnection attempt
            // setTimeout(connectToNative, 5000); // e.g., try again in 5 seconds
        });

        console.log("Native messaging port connection initiated.");

    } catch (error) {
        console.error("Error connecting to native host:", error);
        port = null;
        initialConnectionAttempted = false;
    }
}

// Add connection status check
function isConnected() {
    return port !== null; // Simpler check
}

// Initial connection
console.log("Background script starting...");
connectToNative();

// Handle content script injection and scraping
async function injectAndScrape(tabId, config) {
    try {
        // First, inject the pre-scrape code if it exists
        if (config.pre_scrape_js) {
            await chrome.scripting.executeScript({
                target: { tabId },
                func: (preScrapingCode) => {
                    try {
                        eval(preScrapingCode);
                    } catch (e) {
                        console.error('Pre-scrape script error:', e);
                    }
                },
                args: [config.pre_scrape_js]
            });
        }

        // Wait for the specified timeout
        await new Promise(resolve => setTimeout(resolve, config.timeout_ms));

        // First, capture the raw HTML
        const htmlResults = await chrome.scripting.executeScript({
            target: { tabId },
            func: () => {
                return {
                    rawHtml: document.documentElement.outerHTML,
                    title: document.title,
                    url: window.location.href
                };
            }
        });
        
        const pageInfo = htmlResults[0]?.result || { rawHtml: "", title: "", url: "" };
        console.log("Captured page info:", { title: pageInfo.title, url: pageInfo.url, htmlLength: pageInfo.rawHtml.length });

        // Then do the element scraping
        const results = await chrome.scripting.executeScript({
            target: { tabId },
            func: (scrapeConfig) => {
                const items = document.querySelectorAll(scrapeConfig.item_selector);
                return Array.from(items).map(item => {
                    const result = {};
                    for (const sel of scrapeConfig.selectors) {
                        const element = item.querySelector(sel.selector);
                        if (element) {
                            let value = sel.attribute ? 
                                element.getAttribute(sel.attribute) : 
                                element.textContent;
                            
                            if (sel.post_processing.includes('trim')) {
                                value = value.trim();
                            }
                            result[sel.name] = value;
                        }
                    }
                    return result;
                });
            },
            args: [config]
        });

        // Combine both results
        const scrapedItems = results[0]?.result || [];
        
        // Return the HTML and the scraped items
        return [{
            pageInfo: pageInfo,
            items: scrapedItems
        }];
    } catch (error) {
        console.error("Scraping error:", error);
        throw error;
    }
}

// Handle tasks from the native host
async function handleTask(message) {
    const taskId = message.task_id;
    let currentTabId = null; // Initialize tab ID for this task
    const results = [];

    try {
        console.log(`Handling task ${taskId}:`, message.task);

        for (const step of message.task.steps) {
            let stepResult = {
                type: step.type,
                success: false,
                data: null,
                error: null
            };

            try {
                console.log(`Task ${taskId}, Step ${step.type}: Starting...`);

                if (step.type === 'navigate') {
                    // Handle navigation directly using chrome.tabs API
                    console.log(`Task ${taskId}, Step navigate: Navigating to:`, step.url);
                    const tab = await chrome.tabs.create({ url: step.url, active: true });
                    currentTabId = tab.id; // Store the new tab ID
                    await waitForTabLoad(currentTabId); // Wait for the tab to load
                    console.log(`Task ${taskId}, Step navigate: Navigation complete for tab ${currentTabId}`);
                    stepResult.success = true;

                } else if (currentTabId) {
                    // For all other step types, execute in the content script of the current tab
                    console.log(`Task ${taskId}, Step ${step.type}: Executing in content script for tab ${currentTabId}`);
                    const stepExecutionResult = await chrome.scripting.executeScript({
                        target: { tabId: currentTabId },
                        func: contentScriptExecutor, // The function defined below handleTask
                        args: [step] // Pass the current step object
                    });

                    // Process result from content script
                    if (stepExecutionResult && stepExecutionResult[0] && stepExecutionResult[0].result) {
                        const result = stepExecutionResult[0].result;
                        if (result.error) {
                            throw new Error(result.error); // Throw error if content script reported one
                        }
                        stepResult.data = result.data; // Store extracted data if any
                        stepResult.success = true;
                        console.log(`Task ${taskId}, Step ${step.type}: Execution successful. Data:`, result.data);

                        // Handle navigation potentially triggered by CLICK
                        if (step.type === 'click' && step.wait_for_nav) {
                            console.log(`Task ${taskId}, Step click: Waiting for navigation after click...`);
                            await waitForTabLoad(currentTabId); // Wait for page load after click
                            console.log(`Task ${taskId}, Step click: Navigation complete.`);
                        }
                    } else {
                        // This case might indicate an issue with the content script itself or injection failure
                        console.error(`Task ${taskId}, Step ${step.type}: Content script execution failed or returned no result.`, stepExecutionResult);
                        throw new Error("Content script execution failed or returned no result.");
                    }
                } else {
                    // If currentTabId is null and the step is not 'navigate', we can't proceed
                    throw new Error(`Cannot execute step type '${step.type}' without an active tab. Ensure 'navigate' is the first step.`);
                }

            } catch (error) {
                console.error(`Task ${taskId}, Step ${step.type}: Error -`, error);
                stepResult.error = error.message || String(error);
                stepResult.success = false; // Ensure success is false on error
            }

            results.push(stepResult);

            // If a step failed, stop processing further steps for this task
            if (!stepResult.success) {
                 console.error(`Task ${taskId}: Step ${step.type} failed. Aborting task.`);
                 break;
            }
        }

        // Send final result back to native host
        console.log(`Task ${taskId}: Completed. Sending results back to native host.`);
        if (port) {
            port.postMessage({
                action: "task_result", // Send task_result *to* the native host
                task_id: taskId,
                success: results.every(r => r.success),
                result: { steps: results },
                error: results.find(r => !r.success)?.error || null
            });
        } else {
             console.error(`Task ${taskId}: Cannot send results, native host disconnected.`);
        }

    } catch (error) {
        // Catch errors from the overall task handling logic (e.g., initial setup)
        console.error(`Task ${taskId}: Unhandled error during task execution -`, error);
         if (port) {
            port.postMessage({
                action: "task_result",
                task_id: taskId,
                success: false,
                result: null,
                error: error.message || String(error)
            });
        } else {
             console.error(`Task ${taskId}: Cannot send error result, native host disconnected.`);
        }
    } finally {
         // Optional: Close the tab? Maybe only if we created it?
         // if (currentTabId) {
         //    chrome.tabs.remove(currentTabId).catch(e => console.log("Error closing tab:", e));
         // }
    }
}

// Helper function to wait for tab load (Example implementation)
function waitForTabLoad(tabId, timeout = 30000) {
    return new Promise((resolve, reject) => {
        const startTime = Date.now();
        const checkTab = () => {
            if (Date.now() - startTime > timeout) {
                return reject(new Error(`Tab ${tabId} did not load within ${timeout}ms`));
            }
            chrome.tabs.get(tabId, (tab) => {
                if (chrome.runtime.lastError) {
                    // Tab might have been closed
                    return reject(chrome.runtime.lastError);
                }
                if (tab.status === 'complete') {
                    // Add a small delay after 'complete' for dynamic content
                    setTimeout(resolve, 500);
                } else {
                    setTimeout(checkTab, 200); // Poll status
                }
            });
        };
        checkTab();
    });
}

console.log("Extension ID:", chrome.runtime.id);

chrome.runtime.getPlatformInfo(function(info) {
    console.log("Platform info:", info);
});

// Ensure connectToNative is called on startup
chrome.runtime.onStartup.addListener(() => {
    console.log("Browser startup detected.");
    connectToNative();
});

// Also connect when the extension is first installed or updated
chrome.runtime.onInstalled.addListener(details => {
     console.log("onInstalled event:", details.reason);
     connectToNative(); // Connect on install/update
});

// This function is injected and executed in the target page's context
async function contentScriptExecutor(step) {
    // Helper: Wait for selector function (basic polling)
    function waitForElement(selector, timeout, state = 'attached') {
        return new Promise((resolve, reject) => {
            const startTime = Date.now();
            const interval = setInterval(() => {
                const element = document.querySelector(selector);
                let conditionMet = false;
                if (state === 'attached') { conditionMet = !!element; }
                else if (state === 'visible') { conditionMet = !!element && (element.offsetWidth > 0 || element.offsetHeight > 0 || element.getClientRects().length > 0); }
                else if (state === 'hidden') { conditionMet = !element || (element.offsetWidth === 0 && element.offsetHeight === 0); }

                if (conditionMet) { clearInterval(interval); resolve(element); }
                else if (Date.now() - startTime > timeout) { clearInterval(interval); reject(new Error(`Timeout waiting for selector "${selector}" (state: ${state}) after ${timeout}ms`)); }
            }, 100);
        });
    }
     function dispatchInputEvents(element) {
         element.dispatchEvent(new Event('input', { bubbles: true, cancelable: true }));
         element.dispatchEvent(new Event('change', { bubbles: true, cancelable: true }));
     }

    try {
        switch (step.type) {
            case 'navigate': return { data: null };
            case 'scrape': {
                 const items = [];
                 document.querySelectorAll(step.config.item_selector).forEach(element => {
                     const itemData = {};
                     step.config.selectors.forEach(sel => {
                         const targetElement = element.querySelector(sel.selector);
                         if (targetElement) {
                             let value = sel.attribute ? targetElement.getAttribute(sel.attribute) : targetElement.textContent;
                             if (sel.post_processing && sel.post_processing.includes('trim')) { value = value.trim(); }
                             itemData[sel.name] = value;
                         } else { itemData[sel.name] = null; }
                     });
                     items.push(itemData);
                 });
                 return { data: items };
             }
            case 'click': {
                const element = await waitForElement(step.selector, step.timeout || 5000, 'visible');
                if (!element) throw new Error(`Element not found or not visible for click: ${step.selector}`);
                element.click();
                return { data: null };
            }
            case 'fill': {
                const element = await waitForElement(step.selector, 5000, 'visible');
                if (!element) throw new Error(`Element not found for fill: ${step.selector}`);
                element.value = step.value;
                 if (step.dispatch_events && step.dispatch_events.length > 0) { dispatchInputEvents(element); }
                return { data: null };
            }
            case 'wait_for_selector': {
                await waitForElement(step.selector, step.timeout, step.state || 'attached');
                return { data: null };
            }
            case 'wait_for_timeout': {
                await new Promise(resolve => setTimeout(resolve, step.timeout));
                return { data: null };
            }
            case 'extract': {
                const element = await waitForElement(step.selector, 5000);
                if (!element) throw new Error(`Element not found for extract: ${step.selector}`);
                let value = null;
                switch (step.target) {
                    case 'text': value = element.innerText; break;
                    case 'html': value = element.innerHTML; break;
                    case 'attribute':
                        if (!step.attribute_name) throw new Error("Missing attribute_name for extract target 'attribute'");
                        value = element.getAttribute(step.attribute_name); break;
                    default: throw new Error(`Unknown extract target: ${step.target}`);
                }
                 const extractedData = {};
                 extractedData[step.variable_name] = value;
                 return { data: extractedData };
            }
            default: throw new Error(`Unsupported step type in content script: ${step.type}`);
        }
    } catch (error) { return { error: error.message || String(error) }; }
}
