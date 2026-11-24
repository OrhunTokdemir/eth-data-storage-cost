// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title BlobAnchor
/// @notice Called in the same type-3 (blob) transaction that carries the proof as blobs.
///         Records the blobs' versioned hashes (KZG commitments) and the proof keccak permanently.
///         The blob DATA is pruned by consensus clients after ~4096 epochs (~18 days); anyone who
///         archived it can later prove it matches these on-chain commitments.
contract BlobAnchor {
    struct Anchor {
        bytes32 dataHash;
        bytes32[] versionedHashes;
        uint64 blockNumber;
    }

    mapping(bytes32 => Anchor) private _anchors;

    event Anchored(bytes32 indexed electionId, bytes32 dataHash, uint256 blobCount);

    error NoBlobs();
    error AlreadyAnchored();

    function anchor(bytes32 electionId, bytes32 dataHash) external {
        Anchor storage a = _anchors[electionId];
        if (a.blockNumber != 0) revert AlreadyAnchored();
        for (uint256 i;; ++i) {
            bytes32 h = blobhash(i);
            if (h == bytes32(0)) break;
            a.versionedHashes.push(h);
        }
        if (a.versionedHashes.length == 0) revert NoBlobs();
        a.dataHash = dataHash;
        a.blockNumber = uint64(block.number);
        emit Anchored(electionId, dataHash, a.versionedHashes.length);
    }

    function get(bytes32 electionId) external view returns (Anchor memory) {
        return _anchors[electionId];
    }
}
