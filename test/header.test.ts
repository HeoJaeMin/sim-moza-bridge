import test from "node:test";
import assert from "node:assert/strict";
import { parsePacketHeader } from "../src/f1/header.ts";
import { F1_25_PACKET_FORMAT, PacketId, PACKET_HEADER_SIZE } from "../src/f1/constants.ts";

test("parsePacketHeader reads the packed F1 25 header", () => {
  const packet = Buffer.alloc(PACKET_HEADER_SIZE);
  packet.writeUInt16LE(F1_25_PACKET_FORMAT, 0);
  packet.writeUInt8(25, 2);
  packet.writeUInt8(1, 3);
  packet.writeUInt8(7, 4);
  packet.writeUInt8(1, 5);
  packet.writeUInt8(PacketId.CarDamage, 6);
  packet.writeBigUInt64LE(1234n, 7);
  packet.writeFloatLE(42.5, 15);
  packet.writeUInt32LE(100, 19);
  packet.writeUInt32LE(101, 23);
  packet.writeUInt8(3, 27);
  packet.writeUInt8(255, 28);

  assert.deepEqual(parsePacketHeader(packet), {
    packetFormat: 2025,
    gameYear: 25,
    gameMajorVersion: 1,
    gameMinorVersion: 7,
    packetVersion: 1,
    packetId: PacketId.CarDamage,
    sessionUID: 1234n,
    sessionTime: 42.5,
    frameIdentifier: 100,
    overallFrameIdentifier: 101,
    playerCarIndex: 3,
    secondaryPlayerCarIndex: 255
  });
});

test("parsePacketHeader returns null for short packets", () => {
  assert.equal(parsePacketHeader(Buffer.alloc(PACKET_HEADER_SIZE - 1)), null);
});
