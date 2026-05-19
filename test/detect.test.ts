import test from "node:test";
import assert from "node:assert/strict";
import { detectGameProfileFromPacket } from "../src/detect.ts";
import { F1_25_PACKET_FORMAT, PacketId, PACKET_HEADER_SIZE } from "../src/f1/constants.ts";

function makeHeaderPacket(packetFormat = F1_25_PACKET_FORMAT, gameYear = 25, packetId = PacketId.CarDamage): Buffer {
  const packet = Buffer.alloc(PACKET_HEADER_SIZE);
  packet.writeUInt16LE(packetFormat, 0);
  packet.writeUInt8(gameYear, 2);
  packet.writeUInt8(packetId, 6);
  return packet;
}

test("detectGameProfileFromPacket recognizes F1 25 packets", () => {
  assert.equal(detectGameProfileFromPacket(makeHeaderPacket())?.id, "f1-25");
});

test("detectGameProfileFromPacket ignores malformed or unknown packets", () => {
  assert.equal(detectGameProfileFromPacket(Buffer.from([1, 2, 3])), null);
  assert.equal(detectGameProfileFromPacket(makeHeaderPacket(2024, 24)), null);
  assert.equal(detectGameProfileFromPacket(makeHeaderPacket(F1_25_PACKET_FORMAT, 25, 99)), null);
});
