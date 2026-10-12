import type { Diagnostics } from "@/api/contract";

// What a feedback report carries besides the text (docs: Security and privacy >
// What a feedback report contains): the page's path without anything after
// `?`, the app version, the browser, the screen size, the last error messages
// the app showed and the IDs of the last requests. Never what a request held.

const KEEP = 10;
const errors: string[] = [];
const requestIds: string[] = [];

function remember(list: string[], value: string): void {
  if (list.at(-1) === value) return;
  list.push(value);
  if (list.length > KEEP) list.shift();
}

/** Called where the app shows an error message to the person. */
export function recordError(message: string): void {
  remember(errors, message.slice(0, 500));
}

/** Called for every answer from the service. */
export function recordRequest(response: Response): void {
  const id = response.headers.get("x-request-id");
  if (id) remember(requestIds, id.slice(0, 100));
}

/** The browser and its main version, such as `Chrome 141`. */
export function browserName(userAgent = navigator.userAgent): string {
  const known: [RegExp, string][] = [
    [/Edg\/(\d+)/, "Edge"],
    [/Firefox\/(\d+)/, "Firefox"],
    [/Chrome\/(\d+)/, "Chrome"],
    [/Version\/(\d+)[^ ]* (?:Mobile\/\S+ )?Safari\//, "Safari"],
  ];
  for (const [pattern, name] of known) {
    const version = pattern.exec(userAgent)?.[1];
    if (version) return `${name} ${version}`;
  }
  return userAgent.slice(0, 100);
}

export function collectDiagnostics(version: string): Required<Diagnostics> {
  return {
    page: window.location.pathname,
    app_version: version,
    browser: browserName(),
    screen: `${String(window.screen.width)}×${String(window.screen.height)}`,
    errors: [...errors],
    request_ids: [...requestIds],
  };
}
