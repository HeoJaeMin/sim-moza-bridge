export const F1_25_PACKET_FORMAT = 2025;
export const PACKET_HEADER_SIZE = 29;
export const MAX_CARS = 22;

export const PacketId = {
  Motion: 0,
  Session: 1,
  LapData: 2,
  Event: 3,
  Participants: 4,
  CarSetups: 5,
  CarTelemetry: 6,
  CarStatus: 7,
  FinalClassification: 8,
  LobbyInfo: 9,
  CarDamage: 10,
  SessionHistory: 11,
  TyreSets: 12,
  MotionEx: 13,
  TimeTrial: 14,
  LapPositions: 15
} as const;

export const packetNames: Record<number, string> = {
  [PacketId.Motion]: "Motion",
  [PacketId.Session]: "Session",
  [PacketId.LapData]: "LapData",
  [PacketId.Event]: "Event",
  [PacketId.Participants]: "Participants",
  [PacketId.CarSetups]: "CarSetups",
  [PacketId.CarTelemetry]: "CarTelemetry",
  [PacketId.CarStatus]: "CarStatus",
  [PacketId.FinalClassification]: "FinalClassification",
  [PacketId.LobbyInfo]: "LobbyInfo",
  [PacketId.CarDamage]: "CarDamage",
  [PacketId.SessionHistory]: "SessionHistory",
  [PacketId.TyreSets]: "TyreSets",
  [PacketId.MotionEx]: "MotionEx",
  [PacketId.TimeTrial]: "TimeTrial",
  [PacketId.LapPositions]: "LapPositions"
};
