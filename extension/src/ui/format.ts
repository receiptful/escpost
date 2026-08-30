// Fixed locale and timezone: the copy in the mockups reads "resets 1 September", and
// a quota that resets on a server clock must not appear to reset a day early because
// the till happens to sit in Auckland.
const DAY = new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "long", timeZone: "UTC" });

export function formatDay(at: number): string {
  return DAY.format(new Date(at));
}
