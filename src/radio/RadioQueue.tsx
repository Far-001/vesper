import type { CSSProperties } from "react";
import type { QueueItem } from "../types";

type Props = {
  open: boolean;
  items: QueueItem[];
  index: number;
  radio: boolean;
  onPlay: (index: number) => void;
};

export function RadioQueue(props: Props) {
  return (
    <aside className={`drawer right ${props.open ? "open" : ""}`}>
      <div className="drawer-head">
        <h2>{props.radio ? "Эфир" : "Очередь"}</h2>
        <span className="count">{props.items.length}</span>
      </div>
      <ul className="track-list">
        {props.items.length === 0 && <li className="empty">Очередь пуста. Включите эфир или трек из библиотеки.</li>}
        {props.items.map((item, index) => (
          <li key={`${item.id}-${index}`}>
            <button className={`row ${index === props.index ? "active" : ""}`} onClick={() => props.onPlay(index)}>
              <span className="row-title">
                <em className="idx">{String(index + 1).padStart(2, "0")}</em>
                {item.title}
              </span>
              <span className="row-meta">
                <span>{item.artist}</span>
                {item.bpm > 0 && <em>{Math.round(item.bpm)} bpm</em>}
              </span>
              <span className="energy" style={{ "--level": `${Math.min(100, item.energy * 280)}%` } as CSSProperties} />
              {item.reason && <span className="reason">{item.reason}</span>}
            </button>
          </li>
        ))}
      </ul>
    </aside>
  );
}
