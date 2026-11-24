// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title EventLogStore
/// @notice Emits the proof bytes in event logs (receipts). Readable via eth_getLogs without
///         decoding calldata. Same history-expiry caveat as calldata; ~8 gas/byte extra for LOG.
contract EventLogStore {
    event Chunk(bytes32 indexed electionId, uint32 indexed index, bytes data);

    function emitChunk(bytes32 electionId, uint32 index, bytes calldata data) external {
        emit Chunk(electionId, index, data);
    }
}
