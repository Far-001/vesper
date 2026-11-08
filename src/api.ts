import { invoke } from "@tauri-apps/api/core";
import type { Bootstrap, QueueItem, Track } from "./types";

export const bootstrap = () => invoke<Bootstrap>("bootstrap");

export const librarySetRoot = (path: string) => invoke<Track[]>("library_set_root", { path });

export const librarySearch = (query: string) => invoke<Track[]>("library_search", { query });

export const playIds = (ids: number[], index: number) => invoke<void>("play", { ids, index });

export const playQueueIndex = (index: number) => invoke<void>("play_queue_index", { index });

export const toggle = () => invoke<void>("toggle");

export const seek = (secs: number) => invoke<void>("seek", { secs });

export const playNext = () => invoke<void>("play_next");

export const playPrev = () => invoke<void>("play_prev");

export const setVolume = (volume: number) => invoke<void>("set_volume", { volume });

export const setRepeat = (mode: string) => invoke<void>("set_repeat", { mode });

export const setShuffle = (on: boolean) => invoke<void>("set_shuffle", { on });

export const radioStart = (seedId: number | null) => invoke<void>("radio_start", { seedId });

export const eqSet = (bands: number[]) => invoke<void>("eq_set", { bands });

export const eqPreset = (name: string) => invoke<number[]>("eq_preset", { name });

export const trackCover = (id: number) => invoke<string | null>("track_cover", { id });

export type { QueueItem };
