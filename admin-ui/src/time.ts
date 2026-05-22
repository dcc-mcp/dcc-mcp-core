function twoDigit(value: number): string {
  return String(value).padStart(2, '0');
}

function isSameLocalDate(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

function parseTimestamp(value: string | null | undefined): Date | null {
  if (!value) {
    return null;
  }
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

export function formatTime(value: string | null | undefined, now: Date = new Date()): string {
  const date = parseTimestamp(value);
  if (!date) {
    return '-';
  }

  const time = `${twoDigit(date.getHours())}:${twoDigit(date.getMinutes())}:${twoDigit(date.getSeconds())}`;
  if (isSameLocalDate(date, now)) {
    return time;
  }
  return `${twoDigit(date.getMonth() + 1)}/${twoDigit(date.getDate())} ${time}`;
}

export function timestampTitle(value: string | null | undefined): string | undefined {
  const date = parseTimestamp(value);
  return date ? date.toISOString() : undefined;
}
