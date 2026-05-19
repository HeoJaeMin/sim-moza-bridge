import { PACKET_HEADER_SIZE } from "./constants.ts";

export type PacketHeader = {
  packetFormat: number;
  gameYear: number;
  gameMajorVersion: number;
  gameMinorVersion: number;
  packetVersion: number;
  packetId: number;
  sessionUID: bigint;
  sessionTime: number;
  frameIdentifier: number;
  overallFrameIdentifier: number;
  playerCarIndex: number;
  secondaryPlayerCarIndex: number;
};

export function parsePacketHeader(packet: Buffer): PacketHeader | null {
  if (packet.length < PACKET_HEADER_SIZE) {
    return null;
  }

  return {
    packetFormat: packet.readUInt16LE(0),
    gameYear: packet.readUInt8(2),
    gameMajorVersion: packet.readUInt8(3),
    gameMinorVersion: packet.readUInt8(4),
    packetVersion: packet.readUInt8(5),
    packetId: packet.readUInt8(6),
    sessionUID: packet.readBigUInt64LE(7),
    sessionTime: packet.readFloatLE(15),
    frameIdentifier: packet.readUInt32LE(19),
    overallFrameIdentifier: packet.readUInt32LE(23),
    playerCarIndex: packet.readUInt8(27),
    secondaryPlayerCarIndex: packet.readUInt8(28)
  };
}
