// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title CalldataStore
/// @notice The proof bytes live only in transaction calldata (history), not in state.
///         A small indexed event makes the txs discoverable via eth_getLogs, and its keccak lets
///         a reader check each chunk. Cheapest "on-L1" option, but subject to EIP-4444 history expiry
///         on regular nodes (archive nodes / Portal Network keep it).
contract CalldataStore {
    event ChunkPosted(bytes32 indexed electionId, uint32 indexed index, bytes32 chunkHash, uint32 length);

    function post(bytes32 electionId, uint32 index, bytes calldata chunk) external {
        emit ChunkPosted(electionId, index, keccak256(chunk), uint32(chunk.length));
    }
}
