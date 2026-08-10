# Identity and bootstrap

Preserve:

- local Nostr identity generation;
- one-use recovery;
- acknowledgment before persistence;
- local secret import;
- OS-keyring custody;
- activation, switching, sign-out, removal, and repair.

Identity and signer binding are separate.

Current signer binding:

```text
LocalKeyring
```

Future:

```text
RemoteNip46
ReadOnly
```

Future variants are not implemented during foundation completion.
