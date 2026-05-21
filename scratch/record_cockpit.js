const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');

(async () => {
    // Create output directory if it doesn't exist
    const outputDir = path.join(__dirname, 'recordings');
    if (!fs.existsSync(outputDir)){
        fs.mkdirSync(outputDir, { recursive: true });
    }

    console.log('🚀 Starting Korg premium video recorder...');
    const browser = await chromium.launch({ headless: false });
    const context = await browser.newContext({
        viewport: { width: 1920, height: 1080 },
        deviceScaleFactor: 2 // 4K density recording
    });

    const page = await context.newPage();
    console.log('🔗 Connecting to live local Korg cockpit at http://localhost:8080...');
    await page.goto('http://localhost:8080');
    await page.waitForTimeout(2000); // Allow initial telemetry pulse to render

    // Take clean static state screenshot
    console.log('📸 Capturing clean dashboard state...');
    await page.screenshot({ path: path.join(outputDir, '01_clean_dashboard.png') });

    // Simulate clicking a playhead transaction to trigger inline steering actions
    console.log('⚡ Triggering Playhead Steering Fork action inline...');
    // Force trigger playhead fork action modal inside zero-overlap panel
    await page.evaluate(() => {
        if (typeof openForkModal === 'function') {
            openForkModal(3);
        }
    });

    // Wait for beautiful CSS drawer transition to finish
    await page.waitForTimeout(1000);
    console.log('📸 Capturing expanded inline actions panel...');
    await page.screenshot({ path: path.join(outputDir, '02_expanded_inline_drawer.png') });

    console.log('🎉 Recording complete! PNG keyframes saved inside: scratch/recordings/');
    await browser.close();
})();
