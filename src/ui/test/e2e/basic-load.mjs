import { chromium } from 'playwright';
import { fileURLToPath } from 'url';
import path from 'path';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

async function main() {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();
  
  const errors = [];
  page.on('pageerror', (err) => errors.push(err.message));
  page.on('console', (msg) => {
    if (msg.type() === 'error') errors.push(msg.text());
  });

  // Load the frontend
  await page.goto('http://127.0.0.1:8080/ui/', { waitUntil: 'networkidle', timeout: 15000 });
  
  // Check for JS runtime errors
  if (errors.length > 0) {
    console.log('ERRORS DETECTED:');
    errors.forEach((e) => console.log(`  ❌ ${e}`));
    await browser.close();
    process.exit(1);
  }
  
  // Verify page rendered
  const title = await page.title();
  const bodyText = await page.textContent('body');
  const hasUI = bodyText.includes('Bionic') || bodyText.includes('Chat') || bodyText.includes('New');
  
  console.log(`Title: ${title}`);
  console.log(`UI rendered: ${hasUI ? '✅' : '❌'}`);
  
  if (!hasUI) {
    console.log('Body snippet:', bodyText.substring(0, 200));
    await browser.close();
    process.exit(1);
  }
  
  // Test theme toggle
  const themeBtn = page.locator('button[title*="mode"]').first();
  if (await themeBtn.isVisible()) {
    await themeBtn.click();
    await page.waitForTimeout(300);
    const hasLight = await page.evaluate(() => document.documentElement.classList.contains('light'));
    console.log(`Theme switched to light: ${hasLight ? '✅' : '⚠️'}`);
    
    await themeBtn.click();
    await page.waitForTimeout(300);
  }
  
  console.log('All checks passed ✅');
  await browser.close();
}

main().catch((e) => {
  console.error('Test failed:', e.message);
  process.exit(1);
});
