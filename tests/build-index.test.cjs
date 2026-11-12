const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const script = path.resolve(__dirname, '../scripts/build-index.py');
// Deterministic verifier double: builder tests do not need real Wasm builds or keys.
function fixture() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'registry-builder-'));
  const terra = path.join(dir, 'terra');
  fs.writeFileSync(terra, `#!/usr/bin/env python3
import sys, json
if sys.argv[1] == 'verify':
 print(open(sys.argv[2]).read())
elif sys.argv[1] == 'check-index':
 v=json.load(open(sys.argv[2]))
 assert v['format']==1 and v['expires_ms'] > v['generated_ms']
else:
 sys.exit(1)
`, { mode: 0o700 });
  const artifact = path.join(dir, 'notes.terrapkg');
  const info = { id: 'notes', version: '0.1.0', api: 'terrarium:plugin@0.1.0', name: 'Notes', description: 'Synthetic', icon: 'inbox', signer: 'synthetic-public-key', digest: 'a'.repeat(64), capabilities: ['uses:tasks'] };
  const metadata = path.join(dir, 'metadata.json'), output = path.join(dir, 'index.json');
  const review = { plugins: { notes: { name: 'Notes', description: 'Synthetic', icon: 'inbox', versions: { '0.1.0': { min_host: '0.1.0-alpha.1', yanked: false } } } } };
  const run = (extra = []) => {
    fs.writeFileSync(artifact, JSON.stringify(info)); fs.writeFileSync(metadata, JSON.stringify(review));
    return spawnSync('python3', [script, artifact, '--terra', terra, '--metadata', metadata, '--output', output, '--registry', 'terrarium-official', '--sequence', '1', '--generated-ms', '100', '--expires-ms', '1000', '--origin', 'https://plugins.example.org', ...extra], { encoding: 'utf8' });
  };
  return { dir, info, review, artifact, output, run, close: () => fs.rmSync(dir, { recursive: true, force: true }) };
}
test('index builder produces deterministic unsigned bytes and measures the ZIP', () => {
  const f = fixture(); try {
    assert.equal(f.run().status, 0);
    const raw = fs.readFileSync(f.output, 'utf8'), parsed = JSON.parse(raw);
    assert.ok(raw.endsWith('\n')); assert.equal(parsed.plugins[0].versions[0].size, fs.statSync(f.artifact).size);
    assert.equal(parsed.plugins[0].versions[0].digest, f.info.digest);
    assert.equal(parsed.signature, undefined); assert.equal(f.run().status, 0); assert.equal(fs.readFileSync(f.output, 'utf8'), raw);
  } finally { f.close(); }
});
test('index builder refuses unsigned artifacts, duplicate versions and misleading metadata', () => {
  const f = fixture(); try {
    f.info.signer = null; assert.notEqual(f.run().status, 0);
    f.info.signer = 'synthetic-public-key'; f.review.plugins.notes.name = 'Misleading'; assert.notEqual(f.run().status, 0);
    f.review.plugins.notes.name = 'Notes'; assert.notEqual(f.run([f.artifact]).status, 0);
    assert.equal(fs.existsSync(f.output), false);
  } finally { f.close(); }
});
test('index builder retains history and refuses publisher or immutable-release changes', () => {
  const f = fixture(); try {
    assert.equal(f.run().status, 0); const previous = path.join(f.dir, 'previous.json'); fs.copyFileSync(f.output, previous);
    assert.notEqual(f.run(['--sequence', '2']).status, 0);
    f.info.signer = 'replacement-key'; assert.notEqual(f.run(['--sequence', '2', '--previous', previous]).status, 0);
    f.info.signer = 'synthetic-public-key'; f.info.digest = 'b'.repeat(64); assert.notEqual(f.run(['--sequence', '2', '--previous', previous]).status, 0);
    f.info.digest = 'a'.repeat(64); f.review.plugins.notes.versions['0.1.0'].yanked = true;
    assert.equal(f.run(['--sequence', '2', '--previous', previous]).status, 0);
    assert.equal(JSON.parse(fs.readFileSync(f.output)).plugins[0].versions[0].yanked, true);
    assert.notEqual(f.run(['--sequence', '1', '--previous', previous]).status, 0);
  } finally { f.close(); }
});
