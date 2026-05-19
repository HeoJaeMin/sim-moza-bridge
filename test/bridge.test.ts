import test from "node:test";
import assert from "node:assert/strict";
import { TelemetryBridge } from "../src/bridge.ts";
import { resolveGameProfile } from "../src/games.ts";

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
