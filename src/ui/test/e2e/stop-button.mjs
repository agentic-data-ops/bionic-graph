import { chromium } from 'playwright';
import { fileURLToPath } from 'url';
import path from 'path';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

function sseChunk(content) {
  const data = JSON.stringify({ choices: [{ delta: { content }, index: 0 }] });
  return `data: ${data}\n\n`;
}

async function main() {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();

  const errors = [];
  page.on('pageerror', (err) => errors.push(err.message));
  page.on('console', (msg) => {
    if (msg.type() === 'error') errors.push(msg.text());
  });

  // ── Mock backend endpoints ──
  await page.route('**/maas/openai/v1/models', (route) => {
    route.fulfill({
      status: 200,
      headers: { 'Content-Type': 'application/json', 'x-default-model': 'mock-provider/gpt-4' },
      body: JSON.stringify({ object: 'list', data: [{ id: 'mock-provider/gpt-4', object: 'model', owned_by: 'mock-provider' }] }),
    });
  });
  await page.route('**/graphs', (route) => {
    route.fulfill({ status: 200, body: JSON.stringify({ graphs: ['default'] }) });
  });
  await page.route('**/health', (route) => {
    route.fulfill({ status: 200, body: JSON.stringify({ status: 'ok' }) });
  });
  await page.route('**/settings', (route) => {
    route.fulfill({ status: 200, body: JSON.stringify({}) });
  });

  // Mock chat completion with slow streaming SSE
  await page.route('**/maas/openai/v1/chat/completions', async (route) => {
    const chunks = [
      sseChunk('Hello'),
      sseChunk('! I'),
      sseChunk("'m"),
      sseChunk(' a'),
      sseChunk(' mock'),
      sseChunk(' response'),
      sseChunk('. How'),
      sseChunk(' can I help?'),
      'data: [DONE]\n\n',
    ];
    const encoder = new TextEncoder();
    const stream = new ReadableStream({
      async start(controller) {
        for (const chunk of chunks) {
          await new Promise((r) => setTimeout(r, 200));
          controller.enqueue(encoder.encode(chunk));
        }
        controller.close();
      },
    });
    route.fulfill({
      status: 200,
      headers: { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-cache', Connection: 'keep-alive' },
      body: stream,
    });
  });

  // ── Navigate ──
  await page.goto('http://localhost:4173/ui/', { waitUntil: 'networkidle', timeout: 15000 });

  // Wait for React to mount
  for (let i = 0; i < 20; i++) {
    const hasContent = await page.evaluate(() => document.getElementById('root')?.children?.length > 0);
    if (hasContent) break;
    await page.waitForTimeout(500);
  }

  if (errors.length > 0) {
    console.log('NOTICE — JS errors (pre-existing):');
    errors.forEach((e) => console.log(`  ${e}`));
  }

  // Verify the app actually rendered
  const rootChildren = await page.evaluate(() => document.getElementById('root')?.children?.length || 0);
  if (rootChildren === 0) {
    console.log('❌ React app did not mount');
    await browser.close();
    process.exit(1);
  }
  console.log(`App mounted: ✅ (${rootChildren} children)`);

  // ── Locate elements ──
  const textarea = page.locator('textarea').first();
  const inputRow = textarea.locator('xpath=..');
  const sendButton = inputRow.locator('button').filter({
    has: page.locator('svg path[d="M12 19V5m0 0l-7 7m7-7l7 7"]'),
  });
  const stopButton = inputRow.locator('button').filter({
    has: page.locator('svg rect[x="6"]'),
  });

  // ── Test 1: Initial state — send button visible, stop button NOT visible ──
  console.log('\nTest 1: Initial state');
  const sendBtnVisible = await sendButton.isVisible();
  console.log(`  Send button visible: ${sendBtnVisible ? '✅' : '❌'}`);
  if (!sendBtnVisible) {
    // Debug: dump input row
    const rowHtml = await inputRow.innerHTML();
    console.log('  Input row HTML:', rowHtml.substring(0, 500));
    await browser.close();
    process.exit(1);
  }

  const stopBtnInitCount = await stopButton.count();
  console.log(`  Stop button absent: ${stopBtnInitCount === 0 ? '✅' : '❌'}`);
  if (stopBtnInitCount > 0) {
    console.log('  ❌ Stop button should not be visible initially');
    await browser.close();
    process.exit(1);
  }

  // ── Test 2: Send message → stop button appears ──
  console.log('\nTest 2: Stop button appears during generation');
  await textarea.fill('Test message');
  await sendButton.click();
  // Wait for LLM call to start and isGenerating to become true
  await page.waitForTimeout(600);

  const stopBtnVisible = await stopButton.isVisible();
  console.log(`  Stop button visible: ${stopBtnVisible ? '✅' : '❌'}`);
  if (!stopBtnVisible) {
    console.log('  ❌ Stop button should be visible during generation');
    const rowHtml = await inputRow.innerHTML();
    console.log('  Input row HTML:', rowHtml.substring(0, 500));
    await browser.close();
    process.exit(1);
  }

  // ── Test 3: Click stop → send button returns ──
  console.log('\nTest 3: Stop clicked → send button returns');
  await stopButton.click();
  await page.waitForTimeout(400);

  const sendBtnAfterStop = await sendButton.isVisible();
  console.log(`  Send button visible after stop: ${sendBtnAfterStop ? '✅' : '❌'}`);

  const stopBtnAfterCount = await stopButton.count();
  console.log(`  Stop button gone: ${stopBtnAfterCount === 0 ? '✅' : '❌'}`);

  // ── Test 4: Input is focused after stop ──
  console.log('\nTest 4: Input focus after stop');
  const isFocused = await textarea.evaluate((el) => el === document.activeElement);
  console.log(`  Input has focus: ${isFocused ? '✅' : '❌'}`);

  // ── Summary ──
  const passed = sendBtnVisible && stopBtnInitCount === 0 && stopBtnVisible && sendBtnAfterStop && stopBtnAfterCount === 0 && isFocused;
  console.log(`\n${'='.repeat(40)}`);
  console.log(`All tests ${passed ? 'PASSED ✅' : 'FAILED ❌'}`);
  console.log(`${'='.repeat(40)}`);

  await browser.close();
  if (!passed) process.exit(1);
}

main().catch((e) => {
  console.error('Test failed:', e.message);
  process.exit(1);
});
