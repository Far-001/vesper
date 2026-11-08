export function formatTime(secs: number): string {
  if (!Number.isFinite(secs) || secs < 0) secs = 0;
  const whole = Math.floor(secs);
  const minutes = Math.floor(whole / 60);
  const rest = whole % 60;
  return `${minutes}:${rest.toString().padStart(2, "0")}`;
}
