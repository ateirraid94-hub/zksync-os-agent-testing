# `zks_getProof`

Returns a Merkle proof for a given account storage slot, verifiable against the L1 batch commitment.

## Parameters

| # | Name | Type | Description |
|---|------|------|-------------|
| 1 | `address` | `Address` | The account address. |
| 2 | `keys` | `H256[]` | Array of storage keys to prove. |
| 3 | `l1BatchNumber` | `uint64` | The L1 batch number against which the proof should be generated. The proof is for the state **after** this batch. |

## Response

```json
{
  "address": "0x...",
  "stateCommitmentPreimage": {
    "nextFreeSlot": "0x...",
    "blockNumber": "0x...",
    "last256BlockHashesBlake": "0x...",
    "lastBlockTimestamp": "0x..."
  },
  "storageProofs": [
    {
      "key": "0x...",
      "proof": { ... }
    }
  ]
}
```

### `address`

The account address, as provided in the request. Included in the response so the verifier can derive the flat storage key (`blake2s(address_padded32_be || key)`) without external context.

### `stateCommitmentPreimage`

The preimage fields needed to recompute the L1 state commitment from the Merkle root. These are constant per batch and shared across all storage proofs in the response.

| Field | Type | Description |
|-------|------|-------------|
| `nextFreeSlot` | `uint64` | The next available leaf index in the state tree after this batch. Part of the tree commitment. |
| `blockNumber` | `uint64` | The last L2 block number in this batch. |
| `last256BlockHashesBlake` | `H256` | `blake2s` of the concatenation of the last 256 block hashes (each as 32-byte big-endian). |
| `lastBlockTimestamp` | `uint64` | Timestamp of the last L2 block in this batch. |

### `storageProofs[i]`

Each entry corresponds to one requested storage slot.

| Field | Type | Description |
|-------|------|-------------|
| `key` | `H256` | The storage slot (as provided in the input). The verifier derives the tree key as `blake2s(address_padded32_be || key)`. |
| `proof` | `object` | The proof object. The `type` field discriminates between existing and non-existing proofs (see below). |

The `proof` object always contains a `type` field:

- `"existing"` — the slot exists in the tree. Additional fields: `index`, `value`, `nextIndex`, `siblings`.
- `"nonExisting"` — the slot has never been written to (value is implicitly zero). Additional fields: `leftNeighbor`, `rightNeighbor`.

#### `proof` when `type` = `"existing"`

Returned when the storage slot has been written to at least once.

| Field | Type | Description |
|-------|------|-------------|
| `type` | `string` | `"existing"` |
| `index` | `uint64` | The leaf index in the tree. |
| `value` | `H256` | The storage value. |
| `nextIndex` | `uint64` | The linked-list pointer to the next leaf (by key order). |
| `siblings` | `H256[]` | The Merkle path (see [Siblings](#siblings) below). |

The leaf key used in the tree is not included explicitly — the verifier derives it as `blake2s(address_padded32_be || key)` from the `address` and `key` fields in the response.

#### `proof` when `type` = `"nonExisting"`

Returned when the storage slot has never been written to (value is implicitly zero). Proves non-membership by showing two consecutive leaves in the key-sorted linked list that bracket the queried key.

| Field | Type | Description |
|-------|------|-------------|
| `type` | `string` | `"nonExisting"` |
| `leftNeighbor` | `LeafWithProof` | The leaf with the largest key smaller than the queried key. |
| `rightNeighbor` | `LeafWithProof` | The leaf with the smallest key larger than the queried key. `leftNeighbor.nextIndex` must equal `rightNeighbor.index`. |

#### `LeafWithProof`

Used within non-existing proofs to represent a neighbor leaf and its Merkle path.

| Field | Type | Description |
|-------|------|-------------|
| `index` | `uint64` | The leaf index in the tree. |
| `leafKey` | `H256` | The leaf's key (the `blake2s` derived flat storage key). |
| `value` | `H256` | The leaf's value. |
| `nextIndex` | `uint64` | The linked-list pointer to the next leaf. |
| `siblings` | `H256[]` | The Merkle path (see [Siblings](#siblings) below). |

## Tree Structure

The state tree is a fixed-depth (64) binary Merkle tree using Blake2s-256 as the hash function. Leaves are allocated left-to-right by insertion order and linked together in a sorted linked list by key.

### Key derivation

The flat storage key for a slot is derived as:

```
flat_key = blake2s(address_padded32_be || storage_key)
```

where `address_padded32_be` is the 20-byte address zero-padded on the left to 32 bytes.

### Leaf hashing

```
leaf_hash = blake2s(key || value || next_index_le8)
```

where `key` and `value` are 32 bytes each, and `next_index_le8` is the `next` pointer encoded as 8 bytes little-endian.

An empty (unoccupied) leaf has `key = 0`, `value = 0`, `next = 0`.

### Node hashing

```
node_hash = blake2s(left_child_hash || right_child_hash)
```

### Siblings

The `siblings` array is an ordered list of sibling hashes forming the Merkle path from leaf to root.

**Order.** `siblings[0]` is the sibling at the leaf level (depth 64). Subsequent entries move toward the root. A full (uncompressed) path has 64 entries, with the last entry being the sibling at depth 1 (one level below the root). At each level, if the current index is even the node is a left child; if odd it is a right child. The index is halved (integer division) after each level.

**Empty subtree compression.** The tree has depth 64 but is sparsely populated — most subtrees are entirely empty. The hash of an empty subtree at each level is deterministic:

```
emptyHash[0] = blake2s(0x00{32} || 0x00{32} || 0x00{8})    // empty leaf hash (72 zero bytes)
emptyHash[i] = blake2s(emptyHash[i-1] || emptyHash[i-1])    // for i = 1..63
```

If trailing siblings (toward the root) are equal to the corresponding `emptyHash` for that level, they are omitted. The verifier reconstructs them: if `siblings` has fewer than 64 entries, the missing entries at positions `len(siblings)` through `63` are filled with `emptyHash[len(siblings)]`, `emptyHash[len(siblings)+1]`, etc.

For example, if a leaf is at index 5 in a tree with 100 occupied leaves, siblings at levels ~7 and above will all be empty subtree hashes, so the array will contain only ~7 entries instead of 64.

## Verification

```coq
deriveFlatKey (address, storageKey) → H256 :=
    blake2s(leftPad32(address) || storageKey)

hashLeaf (leafKey, value, nextIndex) → H256 :=
    blake2s(leafKey || value || nextIndex.to_le_bytes(8))

emptyHash (0) → H256 := blake2s(0x00{72})
emptyHash (i) → H256 := blake2s(emptyHash(i-1) || emptyHash(i-1))

padSiblings (siblings) → H256[64] :=
    siblings ++ [emptyHash(i) for i in len(siblings)..63]

walkMerklePath (leafHash, index, siblings) → H256 :=
    fullPath ← padSiblings(siblings)
    current ← leafHash
    idx ← index
    for sibling in fullPath:
        current ← if even(idx) then blake2s(current || sibling)
                                else blake2s(sibling || current)
        idx ← idx / 2
    assert idx = 0
    current

verifyExistingProof (address, storageProof) → (H256, H256) :=
    let flatKey = deriveFlatKey(address, storageProof.key) in
    let p = storageProof.proof in
    let stateRoot = walkMerklePath(hashLeaf(flatKey, p.value, p.nextIndex),
                                   p.index, p.siblings) in
    (stateRoot, p.value)

verifyNonExistingProof (address, storageProof) → (H256, H256) :=
    let flatKey = deriveFlatKey(address, storageProof.key) in
    let left = storageProof.proof.leftNeighbor in
    let right = storageProof.proof.rightNeighbor in
    let leftRoot = walkMerklePath(hashLeaf(left.leafKey, left.value, left.nextIndex),
                                  left.index, left.siblings) in
    let rightRoot = walkMerklePath(hashLeaf(right.leafKey, right.value, right.nextIndex),
                                   right.index, right.siblings) in
    assert leftRoot = rightRoot
    assert left.leafKey < flatKey < right.leafKey
    assert left.nextIndex = right.index
    (leftRoot, 0x00{32})

verifyStateCommitment (stateRoot, preimage, expectedBatchHash) :=
    let stateCommitment = blake2s(
        stateRoot
        || preimage.nextFreeSlot.to_be_bytes(8)
        || preimage.blockNumber.to_be_bytes(8)
        || preimage.last256BlockHashesBlake
        || preimage.lastBlockTimestamp.to_be_bytes(8)
    ) in
    assert stateCommitment = expectedBatchHash
```

### Full verification

```coq
verify (response, batchHash) :=
    forall storageProof in response.storageProofs:
        let (stateRoot, value) =
            match storageProof.proof.type with
            | "existing"    => verifyExistingProof(response.address, storageProof)
            | "nonExisting" => verifyNonExistingProof(response.address, storageProof)
        in
        verifyStateCommitment(stateRoot, response.stateCommitmentPreimage, batchHash)
```

Where `batchHash` is `StoredBatchInfo.batchHash` from L1 for the corresponding batch.

