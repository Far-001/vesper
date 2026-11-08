import { formatTime } from "../format";
import type { Track } from "../types";

type Props = {
  open: boolean;
  tracks: Track[];
  query: string;
  currentId: number | null;
  scan: string;
  onQuery: (value: string) => void;
  onPlay: (track: Track) => void;
  onFolder: () => void;
  onRescan: () => void;
  canRescan: boolean;
};

export function Library(props: Props) {
  return (
    <aside className={`drawer left ${props.open ? "open" : ""}`}>
      <div className="drawer-head">
        <h2>Библиотека</h2>
        <div className="drawer-actions">
          {props.canRescan && (
            <button className="text" onClick={props.onRescan}>
              обновить
            </button>
          )}
          <button className="text on" onClick={props.onFolder}>
            папка
          </button>
        </div>
      </div>
      <input
        className="search"
        value={props.query}
        placeholder="Поиск по названию, исполнителю, альбому"
        onChange={(event) => props.onQuery(event.target.value)}
      />
      {props.scan && <div className="scan">{props.scan}</div>}
      <ul className="track-list">
        {props.tracks.length === 0 && <li className="empty">Нет треков</li>}
        {props.tracks.map((track) => (
          <li key={track.id}>
            <button
              className={`row ${track.id === props.currentId ? "active" : ""}`}
              onClick={() => props.onPlay(track)}
            >
              <span className="row-title">{track.title}</span>
              <span className="row-meta">
                <span>
                  {track.artist}
                  {track.album ? ` · ${track.album}` : ""}
                </span>
                <span className="row-nums">
                  {track.bpm > 0 && <em>{Math.round(track.bpm)}</em>}
                  <span>{formatTime(track.duration)}</span>
                </span>
              </span>
            </button>
          </li>
        ))}
      </ul>
    </aside>
  );
}
