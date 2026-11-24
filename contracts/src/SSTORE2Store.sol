// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

/// @title SSTORE2Store
/// @notice Stores data as the runtime bytecode of tiny "pointer" contracts (SSTORE2 pattern).
///         Runtime code = 0x00 (STOP, so the pointer can't be called) ++ data.
///         Max data per pointer: 24,576 - 1 = 24,575 bytes (EIP-170).
contract SSTORE2Store {
    uint256 public constant MAX_CHUNK = 24_575;

    event PointerCreated(bytes32 indexed electionId, uint32 indexed index, address pointer);

    error ChunkTooLarge();
    error DeployFailed();

    /// @notice Deploy one pointer per chunk. Batch several chunks per tx to save the 21k base cost,
    ///         but keep the tx under the EIP-7825 gas cap (~2-3 full chunks).
    function write(bytes32 electionId, uint32 firstIndex, bytes[] calldata chunks)
        external
        returns (address[] memory pointers)
    {
        pointers = new address[](chunks.length);
        for (uint256 i; i < chunks.length; ++i) {
            pointers[i] = _deploy(chunks[i]);
            emit PointerCreated(electionId, firstIndex + uint32(i), pointers[i]);
        }
    }

    function _deploy(bytes calldata data) internal returns (address pointer) {
        if (data.length > MAX_CHUNK) revert ChunkTooLarge();
        // Creation code (solmate SSTORE2): returns everything after its own 11 bytes as runtime code.
        //   PUSH1 0x0B MSIZE DUP2 CODESIZE SUB DUP1 SWAP3 MSIZE CODECOPY RETURN
        bytes memory creation = abi.encodePacked(hex"600B5981380380925939F3", hex"00", data);
        assembly ("memory-safe") {
            pointer := create(0, add(creation, 0x20), mload(creation))
        }
        if (pointer == address(0)) revert DeployFailed();
    }

    /// @notice Read a pointer's data (skips the 0x00 STOP prefix).
    function read(address pointer) external view returns (bytes memory data) {
        uint256 size = pointer.code.length;
        if (size == 0) return data;
        data = new bytes(size - 1);
        assembly ("memory-safe") {
            extcodecopy(pointer, add(data, 0x20), 1, sub(size, 1))
        }
    }
}
