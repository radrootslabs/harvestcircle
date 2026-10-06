import test from 'node:test';
import assert from 'node:assert/strict';
import {
  decodePublicRecord,
  publicRecordSnapshot
} from '../../src/lib/persistence/records.ts';
await test('actual bounded record codec revalidates stored public strings without opening storage or adopting a foreign author', () => {
  const owner =
    '79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798';
  const peer =
    'c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5';
  const id = '3d030bf5-901d-45e1-8251-41cbdf805e96';
  const row = {
    schema: 1,
    family: 'public_draft',
    owner,
    id,
    revision: 2,
    savedAtMilliseconds: 1000,
    form: {
      title: '',
      description: 'incomplete public description',
      location: '',
      amount: '.',
      currency: '',
      unit: '',
      quantity: '',
      contactType: '',
      contactValue: ''
    }
  };
  const original = Object.getOwnPropertyDescriptor(globalThis, 'indexedDB');
  Object.defineProperty(globalThis, 'indexedDB', {
    configurable: true,
    get() {
      throw new Error('No IDB capability may be acquired by a codec');
    }
  });
  try {
    const ready = decodePublicRecord(JSON.stringify(row), owner, id);
    if (!ready.ok) throw new Error(ready.reason);
    assert.equal(ready.ok, true);
    assert.deepEqual(publicRecordSnapshot(ready.value, owner, id), row);
    assert.equal(publicRecordSnapshot(ready.value, peer, id), undefined);
    assert.deepEqual(decodePublicRecord(JSON.stringify(row), peer, id), {
      ok: false,
      reason: 'owner_mismatch'
    });
  } finally {
    if (original) Object.defineProperty(globalThis, 'indexedDB', original);
    else Reflect.deleteProperty(globalThis, 'indexedDB');
  }
});
