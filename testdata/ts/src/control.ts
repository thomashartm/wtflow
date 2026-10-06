export function control(items: string[]): void {
  for (const item of items) {
    if (item === "skip") { continue; }
    send(item);
  }
  return;
}
declare function send(item: string): void;
