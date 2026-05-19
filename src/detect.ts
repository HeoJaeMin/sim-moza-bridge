import { F1_25_PACKET_FORMAT, PacketId } from "./f1/constants.ts";
import { parsePacketHeader } from "./f1/header.ts";
import { resolveGameProfile } from "./games.ts";
import type { GameProfile } from "./games.ts";

export function detectGameProfileFromPacket(packet: Buffer): GameProfile | null {
  const header = parsePacketHeader(packet);
  if (!header) {
    return null;
  }

  const isKnownF1Packet =
    header.packetFormat === F1_25_PACKET_FORMAT &&
    header.gameYear === 25 &&
    header.packetId >= PacketId.Motion &&
    header.packetId <= PacketId.LapPositions;

  if (isKnownF1Packet) {
    return resolveGameProfile("f1-25");
  }

  return null;
}
