import { BrandMark } from "./BrandMark";

export function LaunchScreen({ label }: { label: string }) {
  return (
    <main className="launch-screen" aria-busy="true" aria-label={label}>
      <BrandMark className="launch-mark" />
      <h1>Aria Focus</h1>
      <span className="launch-pulse" aria-hidden="true" />
    </main>
  );
}
