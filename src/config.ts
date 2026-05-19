import { parseArgs } from "node:util";
import { assertUdpBridgeSupported, resolveGameProfile } from "./games.ts";
import type { GameProfile } from "./games.ts";

export type BridgeMode = "passthrough" | "remap";

export type BridgeConfig = {
  game: GameProfile;
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

function parseOptionalPort(value: string | boolean | undefined, fallback: number, name: string): number {
  if (value === undefined) {
    return fallback;
  }

  return parsePort(value, name);
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
      game: { type: "string", default: "f1-25" },
      "listen-host": { type: "string", default: "0.0.0.0" },
      listen: { type: "string" },
      "moza-host": { type: "string", default: "127.0.0.1" },
      "moza-port": { type: "string" },
      mode: { type: "string", default: "passthrough" },
      "fix-tyre-wear-order": { type: "boolean", default: false },
      "dry-run": { type: "boolean", default: false },
      verbose: { type: "boolean", default: false }
    },
    allowPositionals: false
  });

  const game = resolveGameProfile(String(values.game));
  assertUdpBridgeSupported(game);

  if (Boolean(values["fix-tyre-wear-order"]) && !game.supportsTyreWearOrderFix) {
    throw new Error(`--fix-tyre-wear-order is only supported for game profiles with an F1 wheel-array parser.`);
  }

  return {
    game,
    listenHost: String(values["listen-host"]),
    listenPort: parseOptionalPort(values.listen, game.defaultListenPort ?? 20777, "--listen"),
    mozaHost: String(values["moza-host"]),
    mozaPort: parseOptionalPort(values["moza-port"], game.defaultMozaPort ?? 22025, "--moza-port"),
    mode: parseMode(values.mode),
    fixTyreWearOrder: Boolean(values["fix-tyre-wear-order"]),
    dryRun: Boolean(values["dry-run"]),
    verbose: Boolean(values.verbose)
  };
}
