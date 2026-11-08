export type Track = {
  id: number;
  path: string;
  title: string;
  artist: string;
  album: string;
  duration: number;
  bpm: number;
  energy: number;
  centroid: number;
  analyzed: boolean;
};

export type QueueItem = Track & { reason: string };

export type Playback = {
  trackId: number | null;
  position: number;
  duration: number;
  playing: boolean;
  volume: number;
  repeatMode: "off" | "all" | "one";
  shuffle: boolean;
  radio: boolean;
  queueIndex: number;
  title: string;
  artist: string;
  album: string;
  bpm: number;
  energy: number;
};

export type ScanEvent = {
  phase: string;
  done: number;
  total: number;
  current: string;
};

export type FeaturesEvent = {
  id: number;
  bpm: number;
  energy: number;
  centroid: number;
};

export type Bootstrap = {
  tracks: Track[];
  root: string | null;
  eq: number[];
  volume: number;
  playback: Playback;
  queue: QueueItem[];
  queueIndex: number;
  radio: boolean;
  audioError: string | null;
};

export const emptyPlayback = (): Playback => ({
  trackId: null,
  position: 0,
  duration: 0,
  playing: false,
  volume: 0.85,
  repeatMode: "off",
  shuffle: false,
  radio: false,
  queueIndex: 0,
  title: "",
  artist: "",
  album: "",
  bpm: 0,
  energy: 0,
});
