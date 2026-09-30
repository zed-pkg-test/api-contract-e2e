import assert from 'node:assert/strict';
import test from 'node:test';

const SHA = '5dbda2127357b4be87821902d36e4ce9560f6876';
const BASE = `https://raw.githubusercontent.com/ORESoftware/ores-interfaces/${SHA}/contracts/ores-compose-machine/v1`;
async function text(path) {
  const r = await fetch(`${BASE}/${path}`);
  assert.equal(r.status, 200, path);
  return r.text();
}
const [schemaText, tsp] = await Promise.all([text('authored.schema.json'), text('main.tsp')]);
const defs = JSON.parse(schemaText).$defs;

test('EnsureRequest uses exact snake_case machine admission fields', () => {
  assert.deepEqual(Object.keys(defs.EnsureRequest.properties).sort(), ['project','rebuild','revision','schema_version','service','session'].sort());
  assert.equal(defs.EnsureRequest.additionalProperties, false);
  assert.match(tsp, /schema_version:\s*"ores\.compose\.machine\.v1"/);
});

test('routing labels reject uppercase underscores and edge hyphens', () => {
  const pattern = new RegExp(defs.EnsureRequest.properties.project.pattern);
  for (const good of ['zed-pkg', 'api', 'pr-481']) assert.ok(pattern.test(good), good);
  for (const bad of ['Zed', 'bad_name', '-bad', 'bad-']) assert.equal(pattern.test(bad), false, bad);
});

test('revision grammar rejects control-plane ambiguous refs', () => {
  const pattern = new RegExp(defs.EnsureRequest.properties.revision.pattern);
  for (const good of ['main', 'feature/serial-queue', 'refs/heads/main', 'a1b2c3d4']) assert.ok(pattern.test(good), good);
  for (const bad of ['--help', 'bad ref', 'a..b', 'x@{1}', 'x~1', 'x^1', '/main', 'main.lock']) assert.equal(pattern.test(bad), false, bad);
});

test('optional active object is absent-or-object, never explicit null', () => {
  assert.equal(defs.JobStatusResponse.required.includes('active'), false);
  assert.equal(defs.JobStatusResponse.properties.active.$ref, '#/$defs/ActiveSystem');
  assert.equal(defs.ReadinessResponse.required.includes('active'), false);
  assert.equal(defs.ReadinessResponse.properties.active.$ref, '#/$defs/ActiveSystem');
});

test('machine error vocabulary is typed and closed', () => {
  const codes = defs.MachineErrorResponse.properties.code.anyOf.map((x) => x.const);
  assert.deepEqual(codes, ['invalid_request','unknown_project','unknown_service','lazy_start_denied','queue_full','machine_busy','stale_generation','activation_failed','ingress_unavailable']);
  assert.equal(defs.MachineErrorResponse.additionalProperties, false);
});
