function twoDigit(value: number): string {
  return String(value).padStart(2, '0');
}

export function displayLocale(): string {
  if (typeof document !== 'undefined' && document.documentElement.lang) {
    return document.documentElement.lang;
  }
  if (typeof navigator !== 'undefined' && navigator.language) {
    return navigator.language;
  }
  return 'en-US';
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

export function formatTime(value: string | null | undefined, now: Date = new Date(), locale: string = displayLocale()): string {
  const date = parseTimestamp(value);
  if (!date) {
    return '-';
  }

  const time = locale.startsWith('en-US')
    ? `${twoDigit(date.getHours())}:${twoDigit(date.getMinutes())}:${twoDigit(date.getSeconds())}`
    : new Intl.DateTimeFormat(locale, {
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
        hourCycle: 'h23',
      }).format(date);
  if (isSameLocalDate(date, now)) {
    return time;
  }
  if (locale.startsWith('en-US')) {
    return `${twoDigit(date.getMonth() + 1)}/${twoDigit(date.getDate())} ${time}`;
  }
  const datePart = new Intl.DateTimeFormat(locale, {
    month: '2-digit',
    day: '2-digit',
  }).format(date);
  return `${datePart} ${time}`;
}

export function timestampTitle(value: string | null | undefined): string | undefined {
  const date = parseTimestamp(value);
  return date ? date.toISOString() : undefined;
}
