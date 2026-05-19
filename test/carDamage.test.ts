import test from "node:test";
import assert from "node:assert/strict";
import { F1_25_PACKET_FORMAT, PacketId, PACKET_HEADER_SIZE } from "../src/f1/constants.ts";
import {
  CAR_DAMAGE_PACKET_SIZE,
  carDamageOffset,
  readF1TyreWear,
  rewriteAllTyreWearToMozaNamedOrder,
  rewriteTyreWearToMozaNamedOrder
} from "../src/f1/carDamage.ts";

function makeCarDamagePacket(): Buffer {
  const packet = Buffer.alloc(CAR_DAMAGE_PACKET_SIZE);
  packet.writeUInt16LE(F1_25_PACKET_FORMAT, 0);
  packet.writeUInt8(PacketId.CarDamage, 6);
  return packet;
}

function writeWear(packet: Buffer, carIndex: number, wear: [number, number, number, number]): void {
  const base = carDamageOffset(carIndex);
  packet.writeFloatLE(wear[0], base);
  packet.writeFloatLE(wear[1], base + 4);
  packet.writeFloatLE(wear[2], base + 8);
  packet.writeFloatLE(wear[3], base + 12);
}

function readRawWear(packet: Buffer, carIndex: number): number[] {
  const base = carDamageOffset(carIndex);
  return [
    packet.readFloatLE(base),
    packet.readFloatLE(base + 4),
    packet.readFloatLE(base + 8),
    packet.readFloatLE(base + 12)
  ];
}

test("readF1TyreWear maps F1 25 wheel order RL RR FL FR to named corners", () => {
  const packet = makeCarDamagePacket();
  writeWear(packet, 0, [11, 22, 33, 44]);

  assert.deepEqual(readF1TyreWear(packet, 0), {
    rl: 11,
    rr: 22,
    fl: 33,
    fr: 44
  });
});

test("rewriteTyreWearToMozaNamedOrder rewrites one car from RL RR FL FR to FL FR RL RR", () => {
  const packet = makeCarDamagePacket();
  writeWear(packet, 2, [11, 22, 33, 44]);

  rewriteTyreWearToMozaNamedOrder(packet, 2);

  assert.deepEqual(readRawWear(packet, 2), [33, 44, 11, 22]);
});

test("rewriteAllTyreWearToMozaNamedOrder rewrites all cars and validates packet size", () => {
  const packet = makeCarDamagePacket();
  writeWear(packet, 0, [10, 20, 30, 40]);
  writeWear(packet, 21, [1, 2, 3, 4]);

  assert.equal(rewriteAllTyreWearToMozaNamedOrder(packet), true);
  assert.deepEqual(readRawWear(packet, 0), [30, 40, 10, 20]);
  assert.deepEqual(readRawWear(packet, 21), [3, 4, 1, 2]);
  assert.equal(rewriteAllTyreWearToMozaNamedOrder(Buffer.alloc(PACKET_HEADER_SIZE)), false);
});

test("carDamageOffset rejects invalid car indexes", () => {
  assert.throws(() => carDamageOffset(-1), RangeError);
  assert.throws(() => carDamageOffset(22), RangeError);
});
