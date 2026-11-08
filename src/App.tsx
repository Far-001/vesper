import { getCurrentWindow } from "@tauri-apps/api/window";
import { Deck } from "./deck/Deck";
import { Equalizer } from "./eq/Equalizer";
import { Library } from "./library/Library";
import { RadioQueue } from "./radio/RadioQueue";
import { usePlayer } from "./usePlayer";
import { Spectrum } from "./visualizer/Spectrum";

export function App() {
  const player = usePlayer();
  const win = getCurrentWindow();
  const showHero = player.tracks.length === 0 && !player.libOpen;

  return (
    <div className="app">
      <Spectrum bandsRef={player.bandsRef} playingRef={player.playingRef} />
      <div className="vignette" />
      <header className="titlebar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          Vesper
        </div>
        <div className="ether-flag" data-tauri-drag-region>
          {player.playback.radio ? "эфир" : ""}
        </div>
        <div className="winbtns">
          <button onClick={() => void win.minimize()} aria-label="Свернуть">
            –
          </button>
          <button onClick={() => void win.toggleMaximize()} aria-label="Развернуть">
            □
          </button>
          <button className="close" onClick={() => void win.close()} aria-label="Закрыть">
            ×
          </button>
        </div>
      </header>

      {showHero && (
        <div className="hero">
          <p className="eyebrow">локальное ночное радио</p>
          <h1>Соберите эфир из своей музыки</h1>
          <p>Темп, энергия и яркость — без облака и аккаунтов. Спектр живёт на всём окне.</p>
          <button className="ether on hero-btn" onClick={() => void player.pickFolder()}>
            Открыть папку
          </button>
          {player.scan && <div className="scan hero-scan">{player.scan}</div>}
        </div>
      )}

      <Library
        open={player.libOpen}
        tracks={player.tracks}
        query={player.query}
        currentId={player.playback.trackId}
        scan={player.scan}
        onQuery={player.setQuery}
        onPlay={player.playTrack}
        onFolder={() => void player.pickFolder()}
        onRescan={() => void player.rescan()}
        canRescan={Boolean(player.root)}
      />
      <RadioQueue
        open={player.queueOpen}
        items={player.queue}
        index={player.playback.queueIndex}
        radio={player.playback.radio}
        onPlay={player.playQueue}
      />
      {player.eqOpen && <Equalizer bands={player.eq} onChange={player.onEq} onPreset={(name) => void player.onPreset(name)} />}
      <Deck
        playback={player.playback}
        cover={player.cover}
        onToggle={player.togglePlay}
        onPrev={player.prev}
        onNext={player.next}
        onSeek={player.onSeek}
        onVolume={player.onVolume}
        onRadio={player.radio}
        onShuffle={player.shuffle}
        onRepeat={player.cycleRepeat}
        onLibrary={() => player.setLibOpen((open) => !open)}
        onQueue={() => player.setQueueOpen((open) => !open)}
        onEq={() => player.setEqOpen((open) => !open)}
        libraryOpen={player.libOpen}
        queueOpen={player.queueOpen}
        eqOpen={player.eqOpen}
      />
      {player.error && <div className="toast">{player.error}</div>}
    </div>
  );
}
