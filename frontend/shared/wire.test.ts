import { test } from 'node:test';
import assert from 'node:assert/strict';
import { decimalInteger, formatMinor, readJson } from './wire.ts';
import type { ListResponse } from './wire.ts';

test('BIGINT money stays exact at zero, two, and three minor-unit scales', () => {
  assert.equal(formatMinor({ currency: 'JPY', amount_minor: '0' }, 0), 'JPY 0');
  assert.equal(formatMinor({ currency: 'USD', amount_minor: '1234' }, 2), 'USD 12.34');
  assert.equal(formatMinor({ currency: 'KWD', amount_minor: '1234' }, 3), 'KWD 1.234');
  assert.equal(formatMinor({ currency: 'USD', amount_minor: '9223372036854775807' }, 2), 'USD 92233720368547758.07');
  for (const invalid of ['01', '1.0', '1e3', '-1', '+1', ' 1', '9223372036854775808']) {
    assert.throws(() => decimalInteger(invalid));
  }
});

test('the HTTP consumer preserves and reuses mixed-case opaque non-UUID IDs', async () => {
  const fixture = { items: [{ product_id: 'Product/Mixed_Case:one', variant_id: 'Variant+Opaque:not-a-uuid' }], next_cursor: null };
  const previousFetch = globalThis.fetch;
  const requested: string[] = [];
  globalThis.fetch = async (input) => {
    requested.push(String(input));
    return new Response(JSON.stringify(fixture), { headers: { 'Content-Type': 'application/json' } });
  };
  try {
    const products = await readJson<ListResponse<{ product_id: string; variant_id: string }>>('/api/v1/catalog/products');
    assert.equal(products.items[0].product_id, 'Product/Mixed_Case:one');
    assert.equal(products.items[0].variant_id, 'Variant+Opaque:not-a-uuid');
    await readJson(`/api/v1/catalog/products/${encodeURIComponent(products.items[0].product_id)}`);
    assert.equal(requested[1], '/api/v1/catalog/products/Product%2FMixed_Case%3Aone');
  } finally { globalThis.fetch = previousFetch; }
});
