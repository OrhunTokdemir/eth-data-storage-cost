// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title ProofRegistry
/// @notice Write-once manifest per (electionId, kind). Points to where the proof bytes live
///         and commits to them with keccak256, so a verifier can locate + check the proof
///         using only the chain.
contract ProofRegistry {
    enum Kind {
        SSTORE2,
        Calldata,
        Events,
        Slots,
        Blob,
        Commitment
    }

    struct Manifest {
        address writer;
        uint64 createdAt;
        uint32 totalLength;
        uint32 chunkCount;
        bytes32 dataHash; // keccak256(full proof)
        address store; // contract holding/indexing the data
        bytes32[] refs; // SSTORE2: pointer addresses; Blob: versioned hashes; else empty
        string uri; // Commitment: off-chain location (ar://, ipfs://, https://)
    }

    mapping(bytes32 => mapping(Kind => Manifest)) private _manifests;

    event Registered(bytes32 indexed electionId, Kind indexed kind, bytes32 dataHash, uint32 totalLength);

    error AlreadyRegistered();

    function register(
        bytes32 electionId,
        Kind kind,
        uint32 totalLength,
        uint32 chunkCount,
        bytes32 dataHash,
        address store,
        bytes32[] calldata refs,
        string calldata uri
    ) external {
        Manifest storage m = _manifests[electionId][kind];
        if (m.createdAt != 0) revert AlreadyRegistered();
        m.writer = msg.sender;
        m.createdAt = uint64(block.timestamp);
        m.totalLength = totalLength;
        m.chunkCount = chunkCount;
        m.dataHash = dataHash;
        m.store = store;
        m.refs = refs;
        m.uri = uri;
        emit Registered(electionId, kind, dataHash, totalLength);
    }

    function get(bytes32 electionId, Kind kind) external view returns (Manifest memory) {
        return _manifests[electionId][kind];
    }
}
