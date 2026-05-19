import { parseArgs } from "node:util";

export type BridgeMode = "passthrough" | "remap";

export type BridgeConfig = {
  listenHost: string;
  listenPort: number;
  mozaHost: string;
  mozaPort: number;
  mode: BridgeMode;
  fixTyreWearOrder: boolean;
  dryRun: boolean;
  verbose: boolean;
};

function parsePort(value: string | boolean | undefined, name: string): number {
  if (typeof value !== "string") {
    throw new Error(`${name} must be a port number`);
  }

  const port = Number(value);
  if (!Number.isInteger(port) || port <= 0 || port > 65535) {
    throw new Error(`${name} must be between 1 and 65535`);
  }

  return port;
}

function parseMode(value: string | boolean | undefined): BridgeMode {
  if (value === "passthrough" || value === "remap") {
    return value;
  }

  throw new Error("--mode must be passthrough or remap");
}

export function readConfig(): BridgeConfig {
  const { values } = parseArgs({
    options: {
      "listen-host": { type: "string", default: "0.0.0.0" },
      listen: { type: "string", default: "20777" },
      "moza-host": { type: "string", default: "127.0.0.1" },
      "moza-port": { type: "string", default: "22025" },
      mode: { type: "string", default: "passthrough" },
      "fix-tyre-wear-order": { type: "boolean", default: false },
      "dry-run": { type: "boolean", default: false },
      verbose: { type: "boolean", default: false }
    },
    allowPositionals: false
  });

  return {
    listenHost: String(values["listen-host"]),
    listenPort: parsePort(values.listen, "--listen"),
    mozaHost: String(values["moza-host"]),
    mozaPort: parsePort(values["moza-port"], "--moza-port"),
    mode: parseMode(values.mode),
    fixTyreWearOrder: Boolean(values["fix-tyre-wear-order"]),
    dryRun: Boolean(values["dry-run"]),
    verbose: Boolean(values.verbose)
  };
}
