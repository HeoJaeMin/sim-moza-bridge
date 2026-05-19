import { MAX_CARS, PACKET_HEADER_SIZE } from "./constants.ts";

export const CAR_DAMAGE_DATA_SIZE = 46;
export const CAR_DAMAGE_PACKET_SIZE = PACKET_HEADER_SIZE + MAX_CARS * CAR_DAMAGE_DATA_SIZE;

export type TyreWearByCorner = {
  fl: number;
  fr: number;
  rl: number;
  rr: number;
};

export function isCarDamagePacketSize(packet: Buffer): boolean {
  return packet.length === CAR_DAMAGE_PACKET_SIZE;
}

export function carDamageOffset(carIndex: number): number {
  if (!Number.isInteger(carIndex) || carIndex < 0 || carIndex >= MAX_CARS) {
    throw new RangeError(`carIndex must be between 0 and ${MAX_CARS - 1}`);
  }

  return PACKET_HEADER_SIZE + carIndex * CAR_DAMAGE_DATA_SIZE;
}

export function readF1TyreWear(packet: Buffer, carIndex: number): TyreWearByCorner {
  const base = carDamageOffset(carIndex);

  return {
    rl: packet.readFloatLE(base),
    rr: packet.readFloatLE(base + 4),
    fl: packet.readFloatLE(base + 8),
    fr: packet.readFloatLE(base + 12)
  };
}

export function rewriteTyreWearToMozaNamedOrder(packet: Buffer, carIndex: number): void {
  const base = carDamageOffset(carIndex);
  const rawRl = packet.readFloatLE(base);
  const rawRr = packet.readFloatLE(base + 4);
  const rawFl = packet.readFloatLE(base + 8);
  const rawFr = packet.readFloatLE(base + 12);

  packet.writeFloatLE(rawFl, base);
  packet.writeFloatLE(rawFr, base + 4);
  packet.writeFloatLE(rawRl, base + 8);
  packet.writeFloatLE(rawRr, base + 12);
}

export function rewriteAllTyreWearToMozaNamedOrder(packet: Buffer): boolean {
  if (!isCarDamagePacketSize(packet)) {
    return false;
  }

  for (let carIndex = 0; carIndex < MAX_CARS; carIndex += 1) {
    rewriteTyreWearToMozaNamedOrder(packet, carIndex);
  }

  return true;
}
