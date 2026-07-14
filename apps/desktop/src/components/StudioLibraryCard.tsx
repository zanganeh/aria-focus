export function StudioLibraryCard({ onOpen }: { onOpen: () => void }) {
  return (
    <button type="button" className="studio-library-card" onClick={onOpen}>
      <strong>Create your focus music</strong>
      <span>Choose a few simple preferences for music made for your focus time.</span>
    </button>
  );
}
