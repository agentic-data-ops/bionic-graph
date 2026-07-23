import { chromium } from 'playwright';

async function main() {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();

  // Mock API endpoints
  await page.route('**/proxy/openai/v1/models', (route) => {
    route.fulfill({ status: 200,
      headers: { 'Content-Type': 'application/json', 'x-default-model': 'mock/gpt-4' },
      body: JSON.stringify({ object: 'list', data: [{ id: 'mock/gpt-4', object: 'model', owned_by: 'mock' }] }),
    });
  });
  await page.route('**/graphs', (route) => {
    route.fulfill({ status: 200, body: JSON.stringify({ graphs: ['graph0'], default: 'graph0', time_travel: { graph0: true } }) });
  });
  await page.route('**/health', (route) => {
    route.fulfill({ status: 200, body: JSON.stringify({ status: 'ok' }) });
  });
  await page.route('**/settings', (route) => {
    route.fulfill({ status: 200, body: JSON.stringify({}) });
  });

  // Mock addVertex / addEdge — catch all /vertices and /edges calls
  let vertexCreated = false, edgeCreated = false;
  page.on('console', (msg) => {
    if (msg.text().includes('Add vertex failed') || msg.text().includes('Add edge failed') || msg.text().includes('vertices') || msg.text().includes('edges')) {
      console.log('  LOG:', msg.text());
    }
  });
  await page.route(/\/vertices(\?|$)/, (route) => {
    console.log('  Intercepted:', route.request().method(), route.request().url());
    if (route.request().method() === 'POST') {
      vertexCreated = true;
      route.fulfill({ status: 200, body: JSON.stringify({ id: 100 }) });
      return;
    }
    route.continue();
  });
  await page.route(/\/edges(\?|$)/, (route) => {
    console.log('  Intercepted:', route.request().method(), route.request().url());
    if (route.request().method() === 'POST') {
      edgeCreated = true;
      route.fulfill({ status: 200, body: JSON.stringify({ id: 200 }) });
      return;
    }
    route.continue();
  });

  // Inject localStorage data via addInitScript (runs before app JS)
  await page.addInitScript(() => {
    const convs = [{
      id: 'test-conv-1',
      title: '乔峰测试',
      messages: [{
        id: 'msg1', type: 'user', content: '乔峰'
      }, {
        id: 'msg2', type: 'search_progress', title: '乔峰',
        steps: [{ icon: '✅', name: 'Search completed', status: 'done' }],
        graphData: {
          success: true,
          data: [
            { type: 'vertex', id: 1, name: '乔峰', keywords: ['丐帮', '降龙十八掌'], labels: ['person'], properties: { skill: '降龙十八掌' } },
            { type: 'vertex', id: 2, name: '段誉', keywords: ['大理'], labels: ['person'], properties: {} },
            { type: 'vertex', id: 3, name: '虚竹', keywords: ['少林'], labels: ['person'], properties: {} },
            { type: 'edge', id: 10, label: '结拜兄弟', source: 1, target: 2, properties: {} },
            { type: 'edge', id: 11, label: '结拜兄弟', source: 1, target: 3, properties: {} },
          ]
        },
        graphName: 'graph0',
        timeTravelEnabled: true,
      }]
    }];
    localStorage.setItem('bgraph-convs', JSON.stringify(convs));
    localStorage.setItem('bgraph-settings', JSON.stringify({
      activeProvider: 'mock', defaultGraph: 'graph0', useGraph: true,
      searchMode: 'keyword', kwSearchMode: 'greedy', chatModel: 'gpt-4',
    }));
  });

  // Navigate — localStorage is already set before app JS runs
  await page.goto('http://localhost:5173/ui/', { waitUntil: 'networkidle', timeout: 15000 });
  await page.waitForTimeout(1500);

  // Wait for app mount
  for (let i = 0; i < 20; i++) {
    const has = await page.evaluate(() => document.getElementById('root')?.children?.length > 0);
    if (has) break;
    await page.waitForTimeout(500);
  }

  // Wait for canvas to appear
  const canvas = page.locator('canvas').first();
  for (let i = 0; i < 20; i++) {
    const vis = await canvas.isVisible().catch(() => false);
    if (vis) break;
    await page.waitForTimeout(500);
  }
  console.log(`Canvas visible: ${await canvas.isVisible().catch(() => false) ? '✅' : '❌'}`);

  // Find add buttons
  const addVertexBtn = page.locator('button').filter({ hasText: '+ Vertex' });
  const addEdgeBtn = page.locator('button').filter({ hasText: '+ Edge' });

  for (let i = 0; i < 10; i++) {
    const v = await addVertexBtn.isVisible().catch(() => false);
    const e = await addEdgeBtn.isVisible().catch(() => false);
    if (v && e) break;
    await page.waitForTimeout(500);
  }

  const av = await addVertexBtn.isVisible().catch(() => false);
  const ae = await addEdgeBtn.isVisible().catch(() => false);
  console.log(`+ Vertex button: ${av ? '✅' : '❌'}`);
  console.log(`+ Edge button: ${ae ? '✅' : '❌'}`);
    if (!av || !ae) {
    const html = await page.evaluate(() => document.body.innerHTML.substring(0, 4000));
    console.log('Body HTML:', html);
    await browser.close();
    process.exit(1);
  }

  // ── Test Add Vertex ──
  console.log('\n--- Add Vertex ---');
  await addVertexBtn.click();
  await page.waitForTimeout(300);
  console.log(`Modal open: ${await page.locator('h3').filter({ hasText: 'Add Vertex' }).isVisible() ? '✅' : '❌'}`);
  await page.locator('input[placeholder="Name"]').fill('TestPerson');
  await page.locator('input[placeholder="Labels (comma-separated)"]').fill('person,test');
  await page.locator('input[placeholder="Keywords (comma-separated)"]').fill('testkeyword');
  await page.locator('input[placeholder="key"]').first().fill('age');
  await page.locator('input[placeholder="value"]').first().fill('25');
  await page.locator('button').filter({ hasText: 'Create' }).click();
  await page.waitForTimeout(500);
  console.log(`Vertex created (API called): ${vertexCreated ? '✅' : '❌'}`);

  // ── Test Add Edge ──
  console.log('\n--- Add Edge ---');
  await addEdgeBtn.click();
  await page.waitForTimeout(300);
  console.log(`Modal open: ${await page.locator('h3').filter({ hasText: 'Add Edge' }).isVisible() ? '✅' : '❌'}`);
  await page.locator('input[placeholder="Edge Label"]').fill('knows');
  const selects = page.locator('select');
  await selects.nth(0).selectOption('1');
  await selects.nth(1).selectOption('2');
  // First key/value pair that's in the edge modal (the edge modal's key/value input)
  const keyInputs = page.locator('input[placeholder="key"]');
  const valInputs = page.locator('input[placeholder="value"]');
  const edgeKey = keyInputs.first();
  await edgeKey.fill('since');
  const edgeVal = valInputs.first();
  await edgeVal.fill('2024');
  await page.locator('button').filter({ hasText: 'Create' }).click();
  await page.waitForTimeout(500);
  console.log(`Edge created (API called): ${edgeCreated ? '✅' : '❌'}`);

  // ── Summary ──
  const passed = av && ae && vertexCreated && edgeCreated;
  console.log(`\n${'='.repeat(40)}`);
  console.log(passed ? 'All tests PASSED ✅' : 'Some tests FAILED ❌');
  console.log(`${'='.repeat(40)}`);

  await browser.close();
  if (!passed) process.exit(1);
}

main().catch((e) => {
  console.error('Test failed:', e.message);
  process.exit(1);
});
