interface Props {
  message: string | null;
  onDismiss: () => void;
}

export function ErrorBanner({ message, onDismiss }: Props) {
  if (!message) return null;

  return (
    <section className="error-banner" role="alert" aria-live="assertive">
      <p>
        <strong>Action failed.</strong> {message}
      </p>
      <button type="button" onClick={onDismiss} aria-label="Dismiss error message">
        Dismiss
      </button>
    </section>
  );
}
