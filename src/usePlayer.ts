import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "./api";
import { emptyPlayback, type FeaturesEvent, type Playback, type QueueItem, type ScanEvent, type Track } from "./types";

export function usePlayer() {
  const [tracks, setTracks] = useState<Track[]>([]);
  const [query, setQuery] = useState("");
  const [playback, setPlayback] = useState<Playback>(emptyPlayback());
  const [queue, setQueue] = useState<QueueItem[]>([]);
  const [root, setRoot] = useState<string | null>(null);
  const [eq, setEq] = useState<number[]>(Array(10).fill(0));
  const [scan, setScan] = useState("");
  const [error, setError] = useState("");
  const [cover, setCover] = useState<string | null>(null);
  const [libOpen, setLibOpen] = useState(false);
  const [queueOpen, setQueueOpen] = useState(false);
  const [eqOpen, setEqOpen] = useState(false);
  const bandsRef = useRef<number[]>(Array(64).fill(0));
  const playingRef = useRef(false);
  const coverCache = useRef(new Map<number, string | null>());
  playingRef.current = playback.playing;

  useEffect(() => {
    let unlisten: Array<() => void> = [];
    let alive = true;
    (async () => {
      try {
        const data = await api.bootstrap();
        if (!alive) return;
        setTracks(data.tracks);
        setRoot(data.root);
        setEq(data.eq.length === 10 ? data.eq : Array(10).fill(0));
        setPlayback(data.playback);
        setQueue(data.queue);
        setLibOpen(data.tracks.length > 0);
        if (data.audioError) setError(data.audioError);
      } catch (err) {
        if (alive) setError(String(err));
      }
      const subs = await Promise.all([
        listen<Playback>("playback", (event) => setPlayback(event.payload)),
        listen<{ bands: number[] }>("spectrum", (event) => {
          bandsRef.current = event.payload.bands;
        }),
        listen<{ items: QueueItem[]; index: number; radio: boolean }>("queue", (event) => {
          setQueue(event.payload.items);
          setPlayback((prev) => ({
            ...prev,
            queueIndex: event.payload.index,
            radio: event.payload.radio,
          }));
          if (event.payload.radio) setQueueOpen(true);
        }),
        listen<ScanEvent>("scan", (event) => {
          const { phase, done, total, current } = event.payload;
          if (phase === "done") {
            setScan("");
            void api.librarySearch("").then(setTracks).catch(() => undefined);
            return;
          }
          const label = phase === "tags" ? "Теги" : "Анализ";
          setScan(`${label} ${done}/${total} · ${current}`);
        }),
        listen<FeaturesEvent>("features", (event) => {
          setTracks((prev) =>
            prev.map((track) =>
              track.id === event.payload.id ? { ...track, ...event.payload, analyzed: true } : track,
            ),
          );
          setQueue((prev) =>
            prev.map((track) =>
              track.id === event.payload.id ? { ...track, ...event.payload, analyzed: true } : track,
            ),
          );
        }),
        listen<{ message: string }>("error", (event) => setError(event.payload.message)),
      ]);
      if (!alive) {
        subs.forEach((stop) => stop());
        return;
      }
      unlisten = subs;
    })();
    return () => {
      alive = false;
      unlisten.forEach((stop) => stop());
    };
  }, []);

  useEffect(() => {
    const handle = window.setTimeout(() => {
      void api.librarySearch(query).then(setTracks).catch((err) => setError(String(err)));
    }, 140);
    return () => window.clearTimeout(handle);
  }, [query]);

  useEffect(() => {
    const id = playback.trackId;
    if (id == null) {
      setCover(null);
      return;
    }
    const cached = coverCache.current.get(id);
    if (cached !== undefined) {
      setCover(cached);
      return;
    }
    let alive = true;
    void api.trackCover(id).then((url) => {
      coverCache.current.set(id, url);
      if (alive) setCover(url);
    });
    return () => {
      alive = false;
    };
  }, [playback.trackId]);

  useEffect(() => {
    if (!error) return;
    const handle = window.setTimeout(() => setError(""), 4600);
    return () => window.clearTimeout(handle);
  }, [error]);

  const pickFolder = useCallback(async () => {
    const picked = await open({ directory: true, multiple: false, title: "Папка с музыкой" });
    if (typeof picked !== "string") return;
    setScan("Читаю папку…");
    setLibOpen(true);
    try {
      const next = await api.librarySetRoot(picked);
      setTracks(next);
      setRoot(picked);
      setQuery("");
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const rescan = useCallback(async () => {
    if (!root) return;
    setScan("Обновляю папку…");
    try {
      const next = await api.librarySetRoot(root);
      setTracks(next);
    } catch (err) {
      setError(String(err));
    }
  }, [root]);

  const playTrack = useCallback(
    (track: Track) => {
      const index = Math.max(0, tracks.findIndex((item) => item.id === track.id));
      void api.playIds(tracks.map((item) => item.id), index).catch((err) => setError(String(err)));
    },
    [tracks],
  );

  const onSeek = useCallback((secs: number) => {
    void api.seek(secs).catch((err) => setError(String(err)));
  }, []);

  const onVolume = useCallback((volume: number) => {
    setPlayback((prev) => ({ ...prev, volume }));
    void api.setVolume(volume).catch((err) => setError(String(err)));
  }, []);

  const onEq = useCallback((bands: number[]) => {
    setEq(bands);
    void api.eqSet(bands).catch((err) => setError(String(err)));
  }, []);

  const onPreset = useCallback(async (name: string) => {
    try {
      const bands = await api.eqPreset(name);
      setEq(bands);
    } catch (err) {
      setError(String(err));
    }
  }, []);

  const cycleRepeat = useCallback(() => {
    const order = ["off", "all", "one"] as const;
    const next = order[(order.indexOf(playback.repeatMode) + 1) % order.length];
    setPlayback((prev) => ({ ...prev, repeatMode: next }));
    void api.setRepeat(next).catch((err) => setError(String(err)));
  }, [playback.repeatMode]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA")) return;
      if (event.code === "Space") {
        event.preventDefault();
        void api.toggle();
      } else if (event.code === "ArrowRight") {
        onSeek(playback.position + 5);
      } else if (event.code === "ArrowLeft") {
        onSeek(Math.max(0, playback.position - 5));
      } else if (event.code === "ArrowUp") {
        event.preventDefault();
        onVolume(Math.min(1, playback.volume + 0.05));
      } else if (event.code === "ArrowDown") {
        event.preventDefault();
        onVolume(Math.max(0, playback.volume - 0.05));
      } else if (event.key === "l" || event.key === "L" || event.key === "д" || event.key === "Д") {
        setLibOpen((open) => !open);
      } else if (event.key === "e" || event.key === "E" || event.key === "у" || event.key === "У") {
        setEqOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onSeek, onVolume, playback.position, playback.volume]);

  return {
    tracks,
    query,
    setQuery,
    playback,
    queue,
    root,
    eq,
    scan,
    error,
    cover,
    libOpen,
    setLibOpen,
    queueOpen,
    setQueueOpen,
    eqOpen,
    setEqOpen,
    bandsRef,
    playingRef,
    pickFolder,
    rescan,
    playTrack,
    onSeek,
    onVolume,
    onEq,
    onPreset,
    cycleRepeat,
    togglePlay: () => void api.toggle().catch((err) => setError(String(err))),
    next: () => void api.playNext().catch((err) => setError(String(err))),
    prev: () => void api.playPrev().catch((err) => setError(String(err))),
    shuffle: () => void api.setShuffle(!playback.shuffle).catch((err) => setError(String(err))),
    radio: () => void api.radioStart(playback.trackId).catch((err) => setError(String(err))),
    playQueue: (index: number) => void api.playQueueIndex(index).catch((err) => setError(String(err))),
  };
}
