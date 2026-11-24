// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {SSTORE2Store} from "../src/SSTORE2Store.sol";
import {CalldataStore} from "../src/CalldataStore.sol";
import {EventLogStore} from "../src/EventLogStore.sol";
import {SlotStore} from "../src/SlotStore.sol";
import {ProofRegistry} from "../src/ProofRegistry.sol";

contract StoresTest is Test {
    bytes32 constant ID = keccak256("election-2026");

    function _data(uint256 len, uint256 seed) internal pure returns (bytes memory d) {
        d = new bytes(len);
        for (uint256 i; i < len; i += 32) {
            bytes32 w = keccak256(abi.encode(seed, i));
            for (uint256 j; j < 32 && i + j < len; ++j) d[i + j] = w[j];
        }
    }

    function test_sstore2_roundtrip() public {
        SSTORE2Store s = new SSTORE2Store();
        bytes[] memory chunks = new bytes[](2);
        chunks[0] = _data(24_575, 1);
        chunks[1] = _data(1000, 2);
        address[] memory ptrs = s.write(ID, 0, chunks);
        assertEq(s.read(ptrs[0]), chunks[0]);
        assertEq(s.read(ptrs[1]), chunks[1]);
    }

    function test_sstore2_rejects_oversize() public {
        SSTORE2Store s = new SSTORE2Store();
        bytes[] memory chunks = new bytes[](1);
        chunks[0] = _data(24_576, 1);
        vm.expectRevert(SSTORE2Store.ChunkTooLarge.selector);
        s.write(ID, 0, chunks);
    }

    function test_slots_roundtrip_unaligned_tail() public {
        SlotStore s = new SlotStore();
        bytes memory a = _data(64, 3);
        bytes memory b = _data(45, 4);
        s.write(ID, 0, a);
        s.write(ID, 2, b);
        assertEq(s.read(ID, 0, 64), a);
        assertEq(s.read(ID, 2, 45), b);
    }

    function test_calldata_and_events_emit() public {
        CalldataStore c = new CalldataStore();
        EventLogStore e = new EventLogStore();
        bytes memory d = _data(5000, 5);
        vm.expectEmit(true, true, false, true);
        emit CalldataStore.ChunkPosted(ID, 0, keccak256(d), 5000);
        c.post(ID, 0, d);
        vm.expectEmit(true, true, false, true);
        emit EventLogStore.Chunk(ID, 0, d);
        e.emitChunk(ID, 0, d);
    }

    function test_registry_write_once() public {
        ProofRegistry r = new ProofRegistry();
        bytes32[] memory refs = new bytes32[](0);
        r.register(ID, ProofRegistry.Kind.Commitment, 330_000, 1, keccak256("p"), address(0), refs, "ar://x");
        assertEq(r.get(ID, ProofRegistry.Kind.Commitment).uri, "ar://x");
        vm.expectRevert(ProofRegistry.AlreadyRegistered.selector);
        r.register(ID, ProofRegistry.Kind.Commitment, 1, 1, bytes32(0), address(0), refs, "");
    }
}
