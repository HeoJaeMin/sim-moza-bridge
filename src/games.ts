export type InputKind = "udp" | "shared-memory" | "plugin-shared-memory";
export type ProtocolKind = "f1-25" | "opaque-udp" | "assetto-corsa-evo" | "le-mans-ultimate";

export type GameProfile = {
  id: string;
  name: string;
  aliases: string[];
  inputKind: InputKind;
  protocol: ProtocolKind;
  defaultListenPort?: number;
  defaultMozaPort?: number;
  supportsUdpBridge: boolean;
  supportsTyreWearOrderFix: boolean;
  notes: string[];
};

export const gameProfiles: GameProfile[] = [
  {
    id: "f1-25",
    name: "F1 25",
    aliases: ["f1", "f125", "f1-2025", "f1-25"],
    inputKind: "udp",
    protocol: "f1-25",
    defaultListenPort: 20777,
    defaultMozaPort: 22025,
    supportsUdpBridge: true,
    supportsTyreWearOrderFix: true,
    notes: [
      "F1 25 emits binary UDP telemetry.",
      "MOZA Pit House expects the F1 25 telemetry port to be 22025."
    ]
  },
  {
    id: "generic-udp",
    name: "Generic UDP passthrough",
    aliases: ["generic", "udp", "raw-udp"],
    inputKind: "udp",
    protocol: "opaque-udp",
    defaultListenPort: 20777,
    defaultMozaPort: 22025,
    supportsUdpBridge: true,
    supportsTyreWearOrderFix: false,
    notes: [
      "Use this when another tool already emits telemetry in a format the target app understands.",
      "Packets are forwarded unchanged and no game-specific parser is applied."
    ]
  },
  {
    id: "ace",
    name: "Assetto Corsa EVO",
    aliases: ["ace", "ac-evo", "ac-evo-early-access", "assetto-corsa-evo"],
    inputKind: "shared-memory",
    protocol: "assetto-corsa-evo",
    supportsUdpBridge: false,
    supportsTyreWearOrderFix: false,
    notes: [
      "Current public integrations point to shared-memory or helper-server access rather than a simple UDP output.",
      "A future adapter needs a Windows shared-memory reader or an external UDP exporter."
    ]
  },
  {
    id: "lmu",
    name: "Le Mans Ultimate",
    aliases: ["lmu", "lu", "le-mans-ultimate"],
    inputKind: "plugin-shared-memory",
    protocol: "le-mans-ultimate",
    supportsUdpBridge: false,
    supportsTyreWearOrderFix: false,
    notes: [
      "MOZA Pit House configures LMU through rF2SharedMemoryMapPlugin64.dll in the game's Bin64/Plugins directory.",
      "A future adapter needs an LMU/rFactor shared-memory reader or an external UDP exporter."
    ]
  }
];

export function listGameProfileIds(): string {
  return gameProfiles.map((profile) => profile.id).join(", ");
}

export function resolveGameProfile(value: string): GameProfile {
  const normalized = value.trim().toLowerCase();
  const profile = gameProfiles.find(
    (candidate) => candidate.id === normalized || candidate.aliases.includes(normalized)
  );

  if (!profile) {
    throw new Error(`Unsupported game "${value}". Supported game profiles: ${listGameProfileIds()}`);
  }

  return profile;
}

export function assertUdpBridgeSupported(profile: GameProfile): void {
  if (profile.supportsUdpBridge) {
    return;
  }

  throw new Error(
    [
      `${profile.name} is not a UDP bridge profile yet.`,
      `Input type: ${profile.inputKind}.`,
      ...profile.notes,
      "Use --game generic-udp only if another tool exports compatible UDP packets for this game."
    ].join(" ")
  );
}
