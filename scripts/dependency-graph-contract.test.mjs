import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const contractDir = resolve(here, '../contracts/dependency-graph-v1');
const golden = JSON.parse(readFileSync(join(contractDir, 'golden.json'), 'utf8'));
const httpCases = JSON.parse(readFileSync(join(contractDir, 'http-cases.json'), 'utf8'));
const yamlPath = join(contractDir, 'representations/diamond.yaml');
const tomlPath = join(contractDir, 'representations/diamond.toml');
const digestPattern = /^sha256:[0-9a-f]{64}$/;
const kindRank = new Map([
  ['runtime', 0],
  ['build', 1],
  ['development', 2],
  ['peer', 3],
  ['tooling', 4],
]);

function canonicalJson(value) {
  if (value === null) return 'null';
  if (typeof value === 'boolean') return value ? 'true' : 'false';
  if (typeof value === 'number') {
    assert.ok(Number.isSafeInteger(value), 'v1 canonical JSON forbids floats and unsafe integers');
    return String(value);
  }
  if (typeof value === 'string') return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`;
  assert.equal(typeof value, 'object');
  return `{${Object.keys(value)
    .sort()
    .map((key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`)
    .join(',')}}`;
}

function semanticDigest(document) {
  const payload = structuredClone(document);
  delete payload.graph_digest;
  return `sha256:${createHash('sha256').update(canonicalJson(payload)).digest('hex')}`;
}

function representationDigest(bytes) {
  return `sha256:${createHash('sha256').update(bytes).digest('hex')}`;
}

function identityKey(identity) {
  return [identity.registry_id, identity.org, identity.name, identity.version].join('\u0000');
}

function nodeKey(node) {
  return [identityKey(node.id), node.artifact_digest ?? '', ...(node.features ?? [])].join('\u0000');
}

function edgeKey(edge) {
  assert.ok(kindRank.has(edge.kind), `unknown dependency kind ${edge.kind}`);
  return [
    identityKey(edge.from),
    identityKey(edge.to),
    String(kindRank.get(edge.kind)).padStart(2, '0'),
    edge.requirement ?? '',
    edge.target ?? '',
    edge.optional ? '1' : '0',
    ...(edge.features ?? []),
  ].join('\u0000');
}

function assertSortedUnique(values, key, label) {
  const keys = values.map(key);
  assert.deepEqual(keys, [...keys].sort(), `${label} must use normative ordering`);
  assert.equal(new Set(keys).size, keys.length, `${label} must not contain exact duplicates`);
}

function validateDocument(name, document) {
  assert.equal(document.schema, 'zpkg/dependency-graph/v1', `${name}: schema`);
  assert.match(document.graph_digest, digestPattern, `${name}: graph digest spelling`);
  assert.equal(semanticDigest(document), document.graph_digest, `${name}: semantic digest`);

  if (document.view === 'declared') {
    assert.ok(document.package, `${name}: declared root package`);
    assert.ok(Array.isArray(document.dependencies), `${name}: declared dependencies`);
    for (const dependency of document.dependencies) {
      assert.ok(dependency.registry_id && dependency.org && dependency.name);
      assert.ok(dependency.requirement && dependency.kind);
      assert.equal('version' in dependency, false, `${name}: declared edge must stay unresolved`);
      assert.equal('selected_version' in dependency, false, `${name}: declared edge must stay unresolved`);
    }
    return;
  }

  assert.equal(document.view, 'resolved', `${name}: supported view`);
  assert.ok(document.provenance?.resolver_version, `${name}: resolver provenance`);
  assert.ok(document.provenance?.target, `${name}: target provenance`);
  assert.match(document.provenance?.lock_digest, digestPattern, `${name}: lock digest`);
  assert.ok(document.provenance.registry_snapshots.length > 0, `${name}: registry snapshots`);
  assertSortedUnique(document.provenance.registry_snapshots, (item) => item.registry_id, `${name}: registry snapshots`);
  assertSortedUnique(document.roots, identityKey, `${name}: roots`);
  assertSortedUnique(document.nodes, nodeKey, `${name}: nodes`);
  assertSortedUnique(document.edges, edgeKey, `${name}: edges`);

  const nodeIds = new Set(document.nodes.map((node) => identityKey(node.id)));
  for (const root of document.roots) {
    assert.ok(nodeIds.has(identityKey(root)), `${name}: root references a node`);
  }
  for (const edge of document.edges) {
    assert.ok(nodeIds.has(identityKey(edge.from)), `${name}: edge source references a node`);
    assert.ok(nodeIds.has(identityKey(edge.to)), `${name}: edge target references a node`);
  }

  if (document.completeness === 'complete') {
    assert.equal(document.parent_graph_digest, undefined, `${name}: complete graph has no parent`);
    assert.equal(document.projection, undefined, `${name}: complete graph has no projection`);
  } else {
    assert.equal(document.completeness, 'projected', `${name}: completeness`);
    assert.match(document.parent_graph_digest, digestPattern, `${name}: projection parent`);
    assert.ok(document.projection && Object.keys(document.projection).length > 0, `${name}: projection spec`);
  }
}

function parseYamlSafely(path) {
  const program = [
    'document = YAML.safe_load(',
    '  File.read(ARGV.fetch(0)),',
    '  permitted_classes: [],',
    '  permitted_symbols: [],',
    '  aliases: false',
    ')',
    'STDOUT.write(JSON.generate(document))',
  ].join("\n");
  const result = spawnSync('ruby', ['-ryaml', '-rjson', '-e', program, path], {
    encoding: 'utf8',
  });
  assert.equal(result.status, 0, `safe YAML parse failed: ${result.stderr}`);
  return JSON.parse(result.stdout);
}

function parseToml(path) {
  const program = [
    'import json, sys, tomllib',
    'with open(sys.argv[1], "rb") as source:',
    '    document = tomllib.load(source)',
    'sys.stdout.write(json.dumps(document, separators=(",", ":")))',
  ].join("\n");
  const result = spawnSync('python3', ['-c', program, path], { encoding: 'utf8' });
  assert.equal(result.status, 0, `TOML parse failed: ${result.stderr}`);
  return JSON.parse(result.stdout);
}

test('all golden documents have stable semantic digests and structural invariants', () => {
  for (const [name, document] of Object.entries(golden)) validateDocument(name, document);
});

test('diamond deduplicates the shared node and registry identity prevents collisions', () => {
  const graph = golden.diamond;
  const globalLeaf = graph.nodes.filter(
    (node) => node.id.registry_id === 'registry:global' && node.id.org === 'shared' && node.id.name === 'leaf',
  );
  assert.equal(globalLeaf.length, 1);
  const incoming = graph.edges.filter((edge) => identityKey(edge.to) === identityKey(globalLeaf[0].id));
  assert.equal(incoming.length, 2, 'diamond must retain both incoming edges');

  const coordinateMatches = graph.nodes.filter(
    (node) => node.id.org === 'shared' && node.id.name === 'leaf' && node.id.version === '2.0.0',
  );
  assert.equal(coordinateMatches.length, 2);
  assert.deepEqual(
    coordinateMatches.map((node) => node.id.registry_id),
    ['registry:global', 'registry:private'],
  );
});

test('cycle fixture is finite and preserves both directed edges', () => {
  assert.equal(golden.cycle.nodes.length, 2);
  assert.equal(golden.cycle.edges.length, 2);
  const directions = new Set(
    golden.cycle.edges.map((edge) => `${identityKey(edge.from)}->${identityKey(edge.to)}`),
  );
  assert.equal(directions.size, 2);
});

test('projection has a parent digest, explicit filter, and a distinct digest', () => {
  assert.equal(golden.projected.parent_graph_digest, golden.diamond.graph_digest);
  assert.notEqual(golden.projected.graph_digest, golden.diamond.graph_digest);
  assert.deepEqual(golden.projected.projection, { kinds: ['runtime', 'build'], max_depth: 1 });
});

test('safe YAML and normalized TOML are lossless projections of canonical JSON', () => {
  assert.deepEqual(parseYamlSafely(yamlPath), golden.diamond);
  assert.deepEqual(parseToml(tomlPath), golden.diamond);
});

test('semantic graph identity is shared while strong representation ETags differ', () => {
  const jsonBytes = Buffer.from(`${JSON.stringify(golden.diamond, null, 2)}\n`);
  const yamlBytes = readFileSync(yamlPath);
  const tomlBytes = readFileSync(tomlPath);
  const etags = [jsonBytes, yamlBytes, tomlBytes].map(representationDigest);
  assert.equal(new Set(etags).size, 3);
  for (const etag of etags) assert.notEqual(etag, golden.diamond.graph_digest);
});

test('HTTP cases preserve negotiation, HEAD, privacy, cache, and limit invariants', () => {
  assert.equal(httpCases.routes.declared, '/v1/packages/{org}/{name}/versions/{version}/dependency-graph?view=declared');
  assert.equal(httpCases.routes.resolution, '/v1/resolutions/{resolution_digest}/dependency-graph');
  assert.deepEqual(Object.keys(httpCases.formats), ['json', 'yaml', 'toml', 'dot', 'mermaid']);
  for (const value of Object.values(httpCases.limits)) assert.ok(Number.isSafeInteger(value) && value > 0);

  const byName = new Map(httpCases.cases.map((item) => [item.name, item]));
  const conflict = byName.get('Accept and format conflict');
  assert.equal(conflict.response.status, 406);
  assert.equal(conflict.response.error_code, 'format_conflict');

  const head = byName.get('HEAD mirrors GET metadata without a body');
  assert.equal(head.response.has_body, false);
  assert.equal(head.response.has_graph_digest_header, true);
  assert.equal(head.response.etag_scope, 'representation');

  const unauthorized = byName.get('unauthorized private resolution is non-enumerable');
  const missing = byName.get('missing resolution matches unauthorized private resolution');
  assert.equal(unauthorized.response.status, httpCases.privacy.denial_status);
  assert.equal(missing.response.status, httpCases.privacy.denial_status);
  assert.equal(unauthorized.response.body_ref, missing.response.body_ref);
  assert.equal(unauthorized.response.cache_control, 'private, no-store');
  assert.equal(missing.response.cache_control, 'private, no-store');
  const denial = JSON.stringify(httpCases.privacy.denial_body);
  for (const field of httpCases.privacy.forbidden_denial_fields) {
    assert.equal(denial.includes(`\"${field}\"`), false, `denial leaks ${field}`);
  }

  const publicCase = byName.get('public immutable JSON resolution');
  assert.equal(publicCase.response.cache_control, 'public, max-age=31536000, immutable');
  assert.equal(publicCase.response.etag_scope, 'representation');

  for (const item of httpCases.cases.filter((entry) => entry.response.limit_kind)) {
    assert.equal(item.response.has_partial_graph, false, `${item.name}: no silent truncation`);
  }

  const encodedLimit = byName.get('encoded representation limit uses payload-too-large');
  assert.equal(encodedLimit.response.status, 413);
  const nodeLimit = byName.get('node limit failure never returns a partial graph');
  assert.equal(nodeLimit.response.status, 422);

  for (const item of httpCases.cases) {
    assert.equal('registry_token' in item.request, false, `${item.name}: global v1 must not receive third-party registry tokens`);
  }
});
