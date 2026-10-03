export type SignalDefinition = {
  id: string;
  name: string;
  service: number;
  pid: number;
  unit: string;
  min: number;
  max: number;
  description: string;
};

export type Sample = {
  elapsed_ns: number;
  wall_time_unix_ns: number;
  source_id: string;
  ecu_id: string | null;
  signal_id: string;
  value: number;
  unit: string;
  quality: "valid" | "stale" | "missing" | "timeout" | "parse_error" | "disconnected" | "suspect";
  origin: "live" | "replay";
  raw_bytes: number[] | null;
};

export type ChartTrace = {
  signalId: string;
  name: string;
  pid?: number;
  unit: string;
  color?: string;
  samples: Sample[];
  current?: Sample;
  sampleRate: string;
};

export type DeviceDescriptor = {
  stable_id: string;
  hardware_family: string;
  firmware: string | null;
  path: string | null;
};

export type ConnectionCapabilities = {
  device: DeviceDescriptor;
  protocol: string;
  read_only: boolean;
  supported_signals: SignalDefinition[];
};

export type Marker = {
  id: number | null;
  elapsed_ns: number;
  wall_time_unix_ns: number;
  label: string;
  note: string;
};

export type RecordingSignal = { id: string; unit: string };

export type RecordingInfo = {
  sample_count: number;
  duration_ns: number;
  signals: RecordingSignal[];
  markers: Marker[];
};

export type PlaybackState = {
  session_id: number;
  position_ns: number;
  duration_ns: number;
  speed: number;
  playing: boolean;
};

export type EngineEvent =
  | { type: "connected"; payload: ConnectionCapabilities }
  | { type: "disconnected"; payload?: null }
  | { type: "sample"; payload: Sample }
  | { type: "playbackSample"; payload: { session_id: number; sample: Sample } }
  | { type: "marker"; payload: Marker }
  | { type: "playbackMarker"; payload: { session_id: number; marker: Marker } }
  | { type: "recordingFailed"; payload: { path: string; sample_count: number } }
  | { type: "error"; payload: string }
  | { type: "playbackStarted"; payload?: null }
  | { type: "playbackFinished"; payload?: null }
  | { type: "playbackState"; payload: PlaybackState };
