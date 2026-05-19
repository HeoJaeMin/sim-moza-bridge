import test from "node:test";
import assert from "node:assert/strict";
import { TelemetryBridge } from "../src/bridge.ts";
import { resolveGameProfile } from "../src/games.ts";
import { F1_25_PACKET_FORMAT, PacketId } from "../src/f1/constants.ts";
import { CAR_DAMAGE_PACKET_SIZE, carDamageOffset } from "../src/f1/carDamage.ts";

function makeF1CarDamagePacket(): Buffer {
  const packet = Buffer.alloc(CAR_DAMAGE_PACKET_SIZE);
  packet.writeUInt16LE(F1_25_PACKET_FORMAT, 0);
  packet.writeUInt8(25, 2);
  packet.writeUInt8(PacketId.CarDamage, 6);
  const base = carDamageOffset(0);
  packet.writeFloatLE(10, base);
  packet.writeFloatLE(20, base + 4);
  packet.writeFloatLE(30, base + 8);
  packet.writeFloatLE(40, base + 12);
  return packet;
}

function readRawWear(packet: Buffer): number[] {
  const base = carDamageOffset(0);
  return [
    packet.readFloatLE(base),
    packet.readFloatLE(base + 4),
    packet.readFloatLE(base + 8),
    packet.readFloatLE(base + 12)
  ];
}

test("generic UDP profile forwards opaque packets without F1 parsing", () => {
  const bridge = new TelemetryBridge({
    game: resolveGameProfile("generic-udp"),
    mode: "passthrough",
    fixTyreWearOrder: false
  });
  const packet = Buffer.from([1, 2, 3]);

  const result = bridge.process(packet);

  assert.equal(result?.packet, packet);
  assert.equal(result?.patched, false);
  assert.equal(bridge.stats.malformed, 0);
  assert.equal(bridge.stats.received, 1);
});

test("F1 profile still rejects malformed packets", () => {
  const bridge = new TelemetryBridge({
    game: resolveGameProfile("f1-25"),
    mode: "passthrough",
    fixTyreWearOrder: false
  });

  assert.equal(bridge.process(Buffer.from([1, 2, 3])), null);
  assert.equal(bridge.stats.malformed, 1);
});

test("auto profile forwards unknown packets without locking out later detection", () => {
  const bridge = new TelemetryBridge({
    game: resolveGameProfile("auto"),
    mode: "remap",
    fixTyreWearOrder: true
  });

  const unknown = Buffer.from([1, 2, 3]);
  assert.equal(bridge.process(unknown)?.packet, unknown);
  assert.equal(bridge.stats.malformed, 0);

  const result = bridge.process(makeF1CarDamagePacket());
  assert.equal(result?.detectedGame?.id, "f1-25");
  assert.equal(result?.patched, true);
  assert.deepEqual(readRawWear(result!.packet), [30, 40, 10, 20]);
});
