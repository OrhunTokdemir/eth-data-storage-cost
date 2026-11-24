// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title SlotStore
/// @notice Baseline: stores the proof in 32-byte storage slots (~22.1k gas per word).
///         Permanent in state but by far the most expensive. Included for comparison only.
contract SlotStore {
    mapping(bytes32 => mapping(uint256 => bytes32)) private _words;

    error Misaligned();

    /// @param startWord word offset of this chunk; chunk length must be a multiple of 32 except the last.
    function write(bytes32 electionId, uint256 startWord, bytes calldata chunk) external {
        uint256 n = (chunk.length + 31) / 32;
        mapping(uint256 => bytes32) storage w = _words[electionId];
        for (uint256 i; i < n; ++i) {
            bytes32 word;
            uint256 off = i * 32;
            assembly ("memory-safe") {
                word := calldataload(add(chunk.offset, off))
            }
            if (off + 32 > chunk.length) {
                // zero the bytes past the end of the chunk (calldataload reads beyond)
                uint256 valid = chunk.length - off;
                word = word & bytes32(~(type(uint256).max >> (valid * 8)));
            }
            w[startWord + i] = word;
        }
    }

    function read(bytes32 electionId, uint256 startWord, uint256 length) external view returns (bytes memory out) {
        out = new bytes(length);
        uint256 n = (length + 31) / 32;
        mapping(uint256 => bytes32) storage w = _words[electionId];
        for (uint256 i; i < n; ++i) {
            bytes32 word = w[startWord + i];
            // `new bytes(length)` reserves ceil(length/32) words, so full-word writes stay in bounds
            assembly ("memory-safe") {
                mstore(add(add(out, 0x20), mul(i, 32)), word)
            }
        }
    }
}
