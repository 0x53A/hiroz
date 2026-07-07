#!/usr/bin/env node
// Headless Chrome runner for the threaded hiroz <-> ROS 2 test.
// Prerequisites: COEP server (python3 serve.py 8083), docker compose stack
// in this directory (zenoh router + ROS 2 Jazzy talker/listener).

import puppeteer from 'puppeteer-core';

const URL = 'http://localhost:8083/test_headless.html';
const TIMEOUT_S = parseInt(process.argv[2] || '60', 10);

const browser = await puppeteer.launch({
  executablePath: '/run/current-system/sw/bin/google-chrome-stable',
  headless: true,
  args: ['--no-sandbox', '--disable-gpu'],
});

const page = await browser.newPage();
page.on('console', msg => console.log(`[${msg.type()}] ${msg.text()}`));
page.on('pageerror', err => console.log(`[ERR] ${err.message}`));

await page.goto(URL, { waitUntil: 'domcontentloaded', timeout: 10000 });
console.log(`--- page loaded, waiting up to ${TIMEOUT_S}s ---`);

const start = Date.now();
let output = '';
while ((Date.now() - start) / 1000 < TIMEOUT_S) {
  await new Promise(r => setTimeout(r, 2000));
  output = await page.evaluate(() =>
    document.getElementById('output')?.innerText || ''
  );
  if (output.includes('Tests complete')) break;
}

if (!output.includes('Tests complete')) {
  console.log('\n=== TIMEOUT - partial output ===');
}

console.log('\n' + output);

const passes = (output.match(/PASS/g) || []).length;
const fails = (output.match(/FAIL/g) || []).length;
console.log(`\n--- ${passes} passed, ${fails} failed ---`);

await browser.close();
process.exit(fails > 0 ? 1 : 0);
