// Lance ui/test/scenarios.html dans Edge ou Chrome headless et rend un code de sortie.
// Aucune dépendance npm : `node ui/test/run.mjs`. Navigateur imposé : variable BROWSER.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const candidates = [
  process.env.BROWSER,
  'C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Microsoft\\Edge\\Application\\msedge.exe',
  'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe',
  process.env.LOCALAPPDATA && join(process.env.LOCALAPPDATA, 'Google\\Chrome\\Application\\chrome.exe'),
].filter(Boolean);
const browser = candidates.find((p) => existsSync(p));
if (!browser) {
  console.error('Aucun navigateur Chromium trouvé (Edge ou Chrome) ; définissez BROWSER.');
  process.exit(2);
}

const page = pathToFileURL(fileURLToPath(new URL('scenarios.html', import.meta.url))).href;
const profile = mkdtempSync(join(tmpdir(), 'eb-ui-test-'));
let dom = '';
try {
  dom = execFileSync(browser, [
    '--headless=new', '--disable-gpu', '--no-first-run', '--no-default-browser-check',
    `--user-data-dir=${profile}`, '--virtual-time-budget=30000', '--dump-dom', page,
  ], { encoding: 'utf8', timeout: 120000, stdio: ['ignore', 'pipe', 'ignore'] });
} catch (e) {
  console.error(`navigateur : ${e.code || e.message}`);
} finally {
  rmSync(profile, { recursive: true, force: true });
}

const m = dom.match(/<pre id="result"[^>]*>([\s\S]*?)<\/pre>/);
const report = m ? m[1].replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&quot;/g, '"').replace(/&amp;/g, '&') : '';
console.log(report || '(aucun résultat : la page n’a pas terminé)');
const done = report.match(/DONE (\d+)\/(\d+)/);
process.exit(done && done[1] === done[2] ? 0 : 1);
