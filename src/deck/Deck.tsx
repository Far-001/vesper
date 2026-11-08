import { formatTime } from "../format";
import type { Playback } from "../types";

type Props = {
  playback: Playback;
  cover: string | null;
  onToggle: () => void;
  onPrev: () => void;
  onNext: () => void;
  onSeek: (secs: number) => void;
  onVolume: (volume: number) => void;
  onRadio: () => void;
  onShuffle: () => void;
  onRepeat: () => void;
  onLibrary: () => void;
  onQueue: () => void;
  onEq: () => void;
  libraryOpen: boolean;
  queueOpen: boolean;
  eqOpen: boolean;
};

const repeatLabel: Record<Playback["repeatMode"], string> = {
  off: "выкл",
  all: "все",
  one: "1",
};

export function Deck(props: Props) {
  const { playback } = props;
  const duration = playback.duration || 0;
  const pct = duration > 0 ? Math.min(100, (playback.position / duration) * 100) : 0;
  const title = playback.title || "Ничего не играет";
  const artist = playback.artist || "Откройте папку или выберите трек";

  return (
    <footer className="deck">
      <div className="cover">
        {props.cover ? <img src={props.cover} alt="" /> : <span>V</span>}
      </div>
      <div className="deck-main">
        <div className="track-line">
          <div className="titles">
            <div className="title">{title}</div>
            <div className="artist">
              {artist}
              {playback.album ? ` · ${playback.album}` : ""}
            </div>
          </div>
          {playback.bpm > 0 && <div className="bpm">{Math.round(playback.bpm)} bpm</div>}
        </div>
        <button
          className="seek"
          onClick={(event) => {
            if (duration <= 0) return;
            const rect = event.currentTarget.getBoundingClientRect();
            const ratio = (event.clientX - rect.left) / rect.width;
            props.onSeek(Math.max(0, Math.min(1, ratio)) * duration);
          }}
          aria-label="Перемотка"
        >
          <span className="seek-fill" style={{ width: `${pct}%` }} />
        </button>
        <div className="times">
          <span>{formatTime(playback.position)}</span>
          <span>{formatTime(duration)}</span>
        </div>
        <div className="transport">
          <button className="icon" onClick={props.onPrev} aria-label="Предыдущий">
            <PrevIcon />
          </button>
          <button className="play" onClick={props.onToggle} aria-label={playback.playing ? "Пауза" : "Играть"}>
            {playback.playing ? <PauseIcon /> : <PlayIcon />}
          </button>
          <button className="icon" onClick={props.onNext} aria-label="Следующий">
            <NextIcon />
          </button>
          <button className={`ether ${playback.radio ? "on" : ""}`} onClick={props.onRadio}>
            Эфир
          </button>
          <button className={`text ${playback.shuffle ? "on" : ""}`} onClick={props.onShuffle}>
            перемешать
          </button>
          <button className={`text ${playback.repeatMode !== "off" ? "on" : ""}`} onClick={props.onRepeat}>
            повтор {repeatLabel[playback.repeatMode]}
          </button>
          <span className="spacer" />
          <label className="volume">
            <span>громкость</span>
            <input
              type="range"
              min={0}
              max={1}
              step={0.01}
              value={playback.volume}
              onChange={(event) => props.onVolume(Number(event.target.value))}
            />
          </label>
          <button className={`text ${props.eqOpen ? "on" : ""}`} onClick={props.onEq}>
            EQ
          </button>
          <button className={`text ${props.libraryOpen ? "on" : ""}`} onClick={props.onLibrary}>
            библиотека
          </button>
          <button className={`text ${props.queueOpen ? "on" : ""}`} onClick={props.onQueue}>
            очередь
          </button>
        </div>
      </div>
    </footer>
  );
}

function PlayIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M8 5.5v13l11-6.5-11-6.5z" fill="currentColor" />
    </svg>
  );
}

function PauseIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M7 5h3.2v14H7zM13.8 5H17v14h-3.2z" fill="currentColor" />
    </svg>
  );
}

function PrevIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M6 5h2v14H6zM18 5.5v13L8 12l10-6.5z" fill="currentColor" />
    </svg>
  );
}

function NextIcon() {
  return (
    <svg viewBox="0 0 24 24" aria-hidden="true">
      <path d="M16 5h2v14h-2zM6 5.5v13L16 12 6 5.5z" fill="currentColor" />
    </svg>
  );
}
