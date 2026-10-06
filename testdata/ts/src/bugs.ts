declare const repo: {save(v: unknown): void};
declare function work(): void;
export function bugs(x: number) {
  repo.save(x);
  repo.save(x);
  switch (x) { case 1: work(); case 2: work(); }
  try { work(); } catch (error) {}
  while (true) { work(); }
  return;
  work();
}
