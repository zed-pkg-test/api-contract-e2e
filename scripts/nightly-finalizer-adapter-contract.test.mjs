import assert from 'node:assert/strict';
import { createHash, createHmac } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const contractUrl = new URL('../contracts/nightly-finalizer-adapter-contract.json', import.meta.url);
const contract = JSON.parse(await readFile(contractUrl, 'utf8'));

const MARKER = /<!-- nightly-interdependency-major:(\{[^\r\n]*\}) -->/g;

function sha256(value) {
  return createHash('sha256').update(value).digest('hex');
}

function hmac(key, value, encoding) {
  return createHmac('sha256', key).update(value).digest(encoding);
}

function markerFor(value) {
  return `<!-- nightly-interdependency-major:${JSON.stringify(value)} -->`;
}

function parseMarker(description) {
  const matches = [...String(description).matchAll(MARKER)];
  assert.equal(matches.length, 1, 'exactly one Linear major marker is required');
  return JSON.parse(matches[0][1]);
}

function verifyMarker(marker, expected) {
  assert.deepEqual(Object.keys(marker).sort(), contract.linear.required_marker_fields);
  assert.deepEqual(marker, expected);
  assert.ok(contract.linear.accepted_dispositions.includes(marker.disposition));
  assert.match(marker.family_fingerprint, /^[a-f0-9]{64}$/);
  assert.match(marker.repository, /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/);
  assert.ok(marker.candidate_major > marker.current_major);
}

function verifyIssue(issue, expected) {
  assert.equal(issue.identifier, contract.linear.issue_identifier);
  assert.equal(issue.archivedAt, null);
  assert.equal(issue.completedAt, null);
  assert.equal(issue.canceledAt, null);
  const marker = parseMarker(issue.description);
  verifyMarker(marker, expected);
  return marker;
}

function encodePathSegment(value) {
  return encodeURIComponent(value).replace(/[!'()*]/g, (character) =>
    `%${character.charCodeAt(0).toString(16).toUpperCase()}`
  );
}

function artifactKey(extension) {
  const r2 = contract.r2;
  const date = new Date(r2.validated_at);
  const datePath = date.toISOString().slice(0, 10).replaceAll('-', '/');
  const timestamp = date.toISOString().replaceAll(':', '-');
  return `${r2.prefix}/${datePath}/${r2.artifact_key}/${timestamp}-${r2.graph_sha256}.${extension}`;
}

function signPut(key, body, contentType) {
  const r2 = contract.r2;
  const url = new URL(
    `/${[r2.bucket, ...key.split('/')].map(encodePathSegment).join('/')}`,
    r2.endpoint
  );
  const bodyHash = sha256(body);
  const amzDate = '20260808T053000Z';
  const dateStamp = amzDate.slice(0, 8);
  const headers = {
    'content-type': contentType,
    host: url.host,
    'if-none-match': '*',
    'x-amz-content-sha256': bodyHash,
    'x-amz-date': amzDate,
    'x-amz-meta-sha256': bodyHash
  };
  const ordered = Object.entries(headers).sort(([left], [right]) => left.localeCompare(right));
  const signedHeaders = ordered.map(([name]) => name).join(';');
  const canonicalHeaders = `${ordered.map(([name, value]) => `${name}:${value}`).join('\n')}\n`;
  const canonicalRequest = [
    'PUT',
    url.pathname,
    '',
    canonicalHeaders,
    signedHeaders,
    bodyHash
  ].join('\n');
  const scope = `${dateStamp}/${r2.region}/s3/aws4_request`;
  const stringToSign = [
    'AWS4-HMAC-SHA256',
    amzDate,
    scope,
    sha256(canonicalRequest)
  ].join('\n');
  const dateKey = hmac(`AWS4${r2.secret_access_key}`, dateStamp);
  const regionKey = hmac(dateKey, r2.region);
  const serviceKey = hmac(regionKey, 's3');
  const signingKey = hmac(serviceKey, 'aws4_request');
  const signature = hmac(signingKey, stringToSign, 'hex');
  return { bodyHash, signature, signedHeaders, url: url.href };
}

class ObjectStoreCanary {
  constructor() {
    this.objects = new Map();
    this.operations = [];
  }

  put(key, body, { fail = false } = {}) {
    this.operations.push('PUT');
    if (fail) throw new Error('synthetic PUT failure');
    if (this.objects.has(key)) return { created: false, status: 412 };
    this.objects.set(key, body);
    return { created: true, status: 200 };
  }

  head(key) {
    this.operations.push('HEAD');
    assert.ok(this.objects.has(key), `missing object ${key}`);
    return { status: 200, sha256: sha256(this.objects.get(key)) };
  }

  delete(key) {
    this.operations.push('DELETE');
    this.objects.delete(key);
  }
}

async function persistPair(store, { failMarkdown = false } = {}) {
  const jsonKey = artifactKey('json');
  const markdownKey = artifactKey('md');
  const created = [];
  try {
    const jsonPut = store.put(jsonKey, contract.r2.json_body);
    if (jsonPut.created) created.push(jsonKey);
    assert.equal(store.head(jsonKey).sha256, sha256(contract.r2.json_body));

    const markdownPut = store.put(markdownKey, contract.r2.markdown_body, { fail: failMarkdown });
    if (markdownPut.created) created.push(markdownKey);
    assert.equal(store.head(markdownKey).sha256, sha256(contract.r2.markdown_body));
    return { durable: true, jsonKey, markdownKey };
  } catch (error) {
    for (const key of created.reverse()) store.delete(key);
    throw error;
  }
}

test('contract is exact-source, public-test-only, and merge-forbidden', () => {
  assert.equal(contract.schema, 'nightly-finalizer-adapter-conformance.v1');
  assert.equal(contract.source.repository, 'ORESoftware/project-registry');
  assert.equal(contract.source.pull_request, 40);
  assert.match(contract.source.revision, /^[a-f0-9]{40}$/);
  assert.equal(contract.merge_forbidden, true);
  assert.deepEqual(contract.safety, {
    production_writes: false,
    default_branch_mutation: false,
    signed_urls: false,
    credential_injection: false,
    test_only: true
  });
});

test('Linear major marker binds the canonical v1 field set and active issue state', () => {
  const expected = contract.linear.expected;
  const issue = {
    identifier: contract.linear.issue_identifier,
    description: `# Disposable test issue\n\n${markerFor(expected)}`,
    archivedAt: null,
    completedAt: null,
    canceledAt: null
  };
  assert.deepEqual(verifyIssue(issue, expected), expected);

  assert.throws(
    () => parseMarker(`${markerFor(expected)}\n${markerFor(expected)}`),
    /exactly one/
  );
  assert.throws(
    () => verifyMarker({ ...expected, note: 'unexpected' }, expected)
  );
  assert.throws(
    () => verifyMarker({ ...expected, dependency: 'zed-pkg/other-client' }, expected)
  );
  assert.throws(
    () => verifyIssue({ ...issue, completedAt: '2026-08-08T05:31:00.000Z' }, expected)
  );
});

test('R2 keys and SigV4 PUT signature independently match the reviewed vector', () => {
  const jsonKey = artifactKey('json');
  assert.equal(jsonKey, contract.r2.expected_json_key);
  assert.equal(sha256(contract.r2.json_body), contract.r2.expected_json_body_sha256);

  const signed = signPut(jsonKey, contract.r2.json_body, 'application/json');
  assert.equal(signed.bodyHash, contract.r2.expected_json_body_sha256);
  assert.equal(signed.signature, contract.r2.expected_put_signature);
  assert.deepEqual(signed.signedHeaders.split(';'), contract.r2.required_signed_headers);
  assert.equal(new URL(signed.url).search, '');
  assert.ok(signed.url.startsWith(`${contract.r2.endpoint}/${contract.r2.bucket}/`));
});

test('successful persistence uses conditional PUT then independent HEAD for both objects', async () => {
  const store = new ObjectStoreCanary();
  const result = await persistPair(store);
  assert.equal(result.durable, true);
  assert.deepEqual(store.operations, contract.r2.success_sequence);
  assert.equal(store.objects.size, 2);
});

test('idempotent replay never overwrites matching existing objects', async () => {
  const store = new ObjectStoreCanary();
  await persistPair(store);
  const before = new Map(store.objects);
  store.operations.length = 0;
  await persistPair(store);
  assert.deepEqual(store.operations, contract.r2.success_sequence);
  assert.deepEqual(store.objects, before);
});

test('a second-object failure rolls back only objects created by that invocation', async () => {
  const fresh = new ObjectStoreCanary();
  await assert.rejects(persistPair(fresh, { failMarkdown: true }), /synthetic PUT failure/);
  assert.deepEqual(fresh.operations, contract.r2.second_write_failure_sequence);
  assert.equal(fresh.objects.size, 0);

  const preexisting = new ObjectStoreCanary();
  const jsonKey = artifactKey('json');
  preexisting.objects.set(jsonKey, contract.r2.json_body);
  await assert.rejects(persistPair(preexisting, { failMarkdown: true }), /synthetic PUT failure/);
  assert.deepEqual(preexisting.operations, ['PUT', 'HEAD', 'PUT']);
  assert.equal(preexisting.objects.get(jsonKey), contract.r2.json_body);
});

test('contract contains no live credential-shaped values or signed URLs', () => {
  const serialized = JSON.stringify(contract);
  const forbidden = [
    /gh[pousr]_[A-Za-z0-9]{20,}/,
    /github_pat_[A-Za-z0-9_]{20,}/,
    /lin_api_[A-Za-z0-9]{20,}/,
    /cfat_[A-Za-z0-9]{20,}/,
    /[?&](?:X-Amz-Signature|Signature|token|access_token)=/i
  ];
  for (const pattern of forbidden) assert.doesNotMatch(serialized, pattern);
  assert.equal(contract.r2.uri_scheme, 'r2');
});
