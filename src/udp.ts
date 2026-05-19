import dgram from "node:dgram";
import type { BridgeConfig } from "./config.ts";
import { TelemetryBridge } from "./bridge.ts";

export async function startUdpBridge(config: BridgeConfig): Promise<void> {
  const receiver = dgram.createSocket("udp4");
  const sender = dgram.createSocket("udp4");
  const bridge = new TelemetryBridge(config);

  receiver.on("message", (packet) => {
    const result = bridge.process(packet);
    if (!result) {
      return;
    }

    if (config.dryRun) {
      return;
    }

    sender.send(result.packet, config.mozaPort, config.mozaHost, (error) => {
      if (error) {
        console.error(`[send-error] ${error.message}`);
        return;
      }

      bridge.markForwarded();
    });
  });

  receiver.on("error", (error) => {
    console.error(`[receiver-error] ${error.message}`);
    receiver.close();
    sender.close();
    process.exitCode = 1;
  });

  sender.on("error", (error) => {
    console.error(`[sender-error] ${error.message}`);
    receiver.close();
    sender.close();
    process.exitCode = 1;
  });

  if (config.verbose) {
    setInterval(() => {
      const { received, forwarded, patched, ignored, malformed } = bridge.stats;
      console.log(
        `[stats] received=${received} forwarded=${forwarded} patched=${patched} ignored=${ignored} malformed=${malformed} packets=${bridge.packetSummary()}`
      );
    }, 1000).unref();
  }

  await new Promise<void>((resolve) => {
    receiver.bind(config.listenPort, config.listenHost, () => {
      resolve();
    });
  });

  console.log(
    [
      `F1 MOZA Bridge listening on ${config.listenHost}:${config.listenPort}`,
      config.dryRun ? "dry-run enabled; packets will not be forwarded" : `forwarding to ${config.mozaHost}:${config.mozaPort}`,
      `mode=${config.mode}`,
      `fixTyreWearOrder=${config.fixTyreWearOrder}`
    ].join("\n")
  );

  process.once("SIGINT", () => {
    console.log("\nStopping F1 MOZA Bridge");
    receiver.close();
    sender.close();
    process.exit(0);
  });
}
