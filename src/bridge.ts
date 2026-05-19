import { F1_25_PACKET_FORMAT, PacketId, packetNames } from "./f1/constants.ts";
import { parsePacketHeader } from "./f1/header.ts";
import { rewriteAllTyreWearToMozaNamedOrder } from "./f1/carDamage.ts";
import { detectGameProfileFromPacket } from "./detect.ts";
import type { BridgeConfig } from "./config.ts";
import type { GameProfile, ProtocolKind } from "./games.ts";

export type BridgeStats = {
  received: number;
  forwarded: number;
  patched: number;
  ignored: number;
  malformed: number;
  byPacketId: Map<number, number>;
};

export type ProcessedPacket = {
  packet: Buffer;
  patched: boolean;
  detectedGame?: GameProfile;
};

export class TelemetryBridge {
  private readonly config: Pick<BridgeConfig, "game" | "mode" | "fixTyreWearOrder">;
  private activeGame: GameProfile;

  readonly stats: BridgeStats = {
    received: 0,
    forwarded: 0,
    patched: 0,
    ignored: 0,
    malformed: 0,
    byPacketId: new Map()
  };

  constructor(config: Pick<BridgeConfig, "game" | "mode" | "fixTyreWearOrder">) {
    this.config = config;
    this.activeGame = config.game;
  }

  process(packet: Buffer): ProcessedPacket | null {
    this.stats.received += 1;

    const detectedGame = this.detectActiveGame(packet);
    const protocol = detectedGame?.protocol ?? this.activeGame.protocol;

    if (protocol === "auto" || protocol === "opaque-udp") {
      return { packet, patched: false, detectedGame };
    }

    const header = this.parseKnownProtocolHeader(packet, protocol);
    if (!header) {
      return null;
    }

    if (this.config.mode === "passthrough") {
      return { packet, patched: false, detectedGame };
    }

    if (header.packetFormat !== F1_25_PACKET_FORMAT) {
      this.stats.ignored += 1;
      return { packet, patched: false, detectedGame };
    }

    if (this.config.fixTyreWearOrder && header.packetId === PacketId.CarDamage) {
      const patchedPacket = Buffer.from(packet);
      const patched = rewriteAllTyreWearToMozaNamedOrder(patchedPacket);

      if (patched) {
        this.stats.patched += 1;
        return { packet: patchedPacket, patched: true, detectedGame };
      }

      this.stats.malformed += 1;
    }

    return { packet, patched: false, detectedGame };
  }

  private detectActiveGame(packet: Buffer): GameProfile | undefined {
    if (this.activeGame.protocol !== "auto") {
      return undefined;
    }

    const detectedGame = detectGameProfileFromPacket(packet);
    if (!detectedGame) {
      return undefined;
    }

    this.activeGame = detectedGame;
    return detectedGame;
  }

  private parseKnownProtocolHeader(packet: Buffer, protocol: ProtocolKind): ReturnType<typeof parsePacketHeader> {
    if (protocol !== "f1-25") {
      this.stats.ignored += 1;
      return null;
    }

    const header = parsePacketHeader(packet);
    if (!header) {
      this.stats.malformed += 1;
      return null;
    }

    this.stats.byPacketId.set(header.packetId, (this.stats.byPacketId.get(header.packetId) ?? 0) + 1);
    return header;
  }

  markForwarded(): void {
    this.stats.forwarded += 1;
  }

  packetSummary(): string {
    const entries = Array.from(this.stats.byPacketId.entries())
      .sort(([a], [b]) => a - b)
      .map(([id, count]) => `${packetNames[id] ?? id}:${count}`)
      .join(" ");

    return entries || "none";
  }
}
