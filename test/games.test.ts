import test from "node:test";
import assert from "node:assert/strict";
import { assertUdpBridgeSupported, resolveGameProfile } from "../src/games.ts";

test("resolveGameProfile accepts canonical ids and aliases", () => {
  assert.equal(resolveGameProfile("auto").id, "auto");
  assert.equal(resolveGameProfile("f1-25").id, "f1-25");
  assert.equal(resolveGameProfile("F125").id, "f1-25");
  assert.equal(resolveGameProfile("ace").id, "ace");
  assert.equal(resolveGameProfile("LU").id, "lmu");
});

test("assertUdpBridgeSupported allows UDP profiles and rejects non-UDP profiles", () => {
  assert.doesNotThrow(() => assertUdpBridgeSupported(resolveGameProfile("auto")));
  assert.doesNotThrow(() => assertUdpBridgeSupported(resolveGameProfile("f1-25")));
  assert.doesNotThrow(() => assertUdpBridgeSupported(resolveGameProfile("generic-udp")));
  assert.throws(() => assertUdpBridgeSupported(resolveGameProfile("ace")), /not a UDP bridge profile/);
  assert.throws(() => assertUdpBridgeSupported(resolveGameProfile("lmu")), /not a UDP bridge profile/);
});

test("unsupported profiles show the supported profile list", () => {
  assert.throws(() => resolveGameProfile("iracing"), /Supported game profiles/);
});
