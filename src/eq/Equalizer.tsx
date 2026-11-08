const LABELS = ["32", "64", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];

const PRESETS: Array<[string, string]> = [
  ["flat", "Ровно"],
  ["rock", "Рок"],
  ["bass", "Бас"],
  ["vocal", "Вокал"],
  ["night", "Ночь"],
];

type Props = {
  bands: number[];
  onChange: (bands: number[]) => void;
  onPreset: (name: string) => void;
};

export function Equalizer({ bands, onChange, onPreset }: Props) {
  const setBand = (index: number, value: number) => {
    const next = bands.slice();
    next[index] = value;
    onChange(next);
  };

  return (
    <section className="eq">
      <div className="eq-presets">
        {PRESETS.map(([id, label]) => (
          <button key={id} className="text" onClick={() => onPreset(id)}>
            {label}
          </button>
        ))}
      </div>
      <div className="eq-bands">
        {LABELS.map((label, index) => (
          <VSlider
            key={label}
            label={label}
            value={bands[index] ?? 0}
            onChange={(value) => setBand(index, value)}
          />
        ))}
      </div>
    </section>
  );
}

function VSlider({ label, value, onChange }: { label: string; value: number; onChange: (value: number) => void }) {
  const setFromY = (clientY: number, target: HTMLDivElement) => {
    const rect = target.getBoundingClientRect();
    const ratio = 1 - (clientY - rect.top) / rect.height;
    const next = -12 + Math.min(1, Math.max(0, ratio)) * 24;
    onChange(Math.round(next * 10) / 10);
  };

  return (
    <div className="vslider">
      <div
        className="vslider-track"
        onPointerDown={(event) => {
          const target = event.currentTarget;
          target.setPointerCapture(event.pointerId);
          setFromY(event.clientY, target);
        }}
        onPointerMove={(event) => {
          if (event.currentTarget.hasPointerCapture(event.pointerId)) {
            setFromY(event.clientY, event.currentTarget);
          }
        }}
        onDoubleClick={() => onChange(0)}
      >
        <span className="vslider-zero" />
        <span className="vslider-fill" style={{ height: `${((value + 12) / 24) * 100}%` }} />
        <span className="vslider-knob" style={{ bottom: `calc(${((value + 12) / 24) * 100}% - 5px)` }} />
      </div>
      <span className="vslider-val">{value > 0 ? `+${value.toFixed(0)}` : value.toFixed(0)}</span>
      <span className="vslider-label">{label}</span>
    </div>
  );
}
