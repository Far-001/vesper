import { useEffect, useRef, type MutableRefObject } from "react";

type Props = {
  bandsRef: MutableRefObject<number[]>;
  playingRef: MutableRefObject<boolean>;
};

const CYAN = [62, 224, 255];
const BLUE = [58, 107, 255];
const MAGENTA = [255, 61, 154];
const LIME = [214, 255, 74];

function mix(a: number[], b: number[], t: number): string {
  const c = a.map((value, i) => Math.round(value + (b[i] - value) * t));
  return `rgb(${c[0]}, ${c[1]}, ${c[2]})`;
}

function colorFor(index: number, height: number): string {
  if (height > 0.92) return `rgb(${LIME[0]}, ${LIME[1]}, ${LIME[2]})`;
  const t = index / 63;
  if (t < 0.45) return mix(CYAN, BLUE, t / 0.45);
  return mix(BLUE, MAGENTA, (t - 0.45) / 0.55);
}

export function Spectrum({ bandsRef, playingRef }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const context = canvas.getContext("2d");
    if (!context) return;
    const peaks = Array(64).fill(0.04);
    let frame = 0;

    const resize = () => {
      const dpr = window.devicePixelRatio || 1;
      const { clientWidth, clientHeight } = canvas;
      canvas.width = Math.max(1, Math.floor(clientWidth * dpr));
      canvas.height = Math.max(1, Math.floor(clientHeight * dpr));
      context.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    resize();
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);

    let raf = 0;
    const draw = (time: number) => {
      const width = canvas.clientWidth;
      const height = canvas.clientHeight;
      const playing = playingRef.current;
      if (playing) {
        context.fillStyle = "rgba(5, 6, 10, 0.22)";
        context.fillRect(0, 0, width, height);
      } else {
        context.fillStyle = "#05060a";
        context.fillRect(0, 0, width, height);
      }

      const bands = bandsRef.current;
      const count = 64;
      const gap = Math.max(2, width * 0.004);
      const barW = Math.max(2, (width - gap * (count + 1)) / count);
      const floor = height * 0.9;
      const maxH = height * 0.74;

      for (let i = 0; i < count; i += 1) {
        let value = playing ? bands[i] ?? 0 : 0.035 + 0.028 * Math.sin(time / 900 + i * 0.22) * Math.sin(time / 1400);
        value = Math.max(0, Math.min(1, value));
        peaks[i] = Math.max(value, peaks[i] * 0.985);
        const color = colorFor(i, value);
        const x = gap + i * (barW + gap);
        const h = Math.max(2, value * maxH);
        context.shadowColor = color;
        context.shadowBlur = playing ? 16 : 0;
        context.fillStyle = color;
        context.globalAlpha = playing ? 0.92 : 0.45;
        context.fillRect(x, floor - h, barW, h);
        context.globalAlpha = 1;
        context.shadowBlur = 0;
        const peakY = floor - Math.max(2, peaks[i] * maxH);
        context.fillStyle = color;
        context.fillRect(x, peakY - 2, barW, 2);
      }

      context.strokeStyle = "rgba(62, 224, 255, 0.18)";
      context.beginPath();
      context.moveTo(0, floor);
      context.lineTo(width, floor);
      context.stroke();
      frame += 1;
      if (frame > 1_000_000) frame = 0;
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => {
      cancelAnimationFrame(raf);
      observer.disconnect();
    };
  }, [bandsRef, playingRef]);

  return <canvas ref={canvasRef} className="spectrum" />;
}
