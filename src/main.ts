#!/usr/bin/env node
import { readConfig } from "./config.ts";
import { startUdpBridge } from "./udp.ts";

try {
  const config = readConfig();
  await startUdpBridge(config);
} catch (error) {
  const message = error instanceof Error ? error.message : String(error);
  console.error(`[startup-error] ${message}`);
  process.exitCode = 1;
}
