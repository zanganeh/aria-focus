export function AboutAriaFocus() {
  return (
    <section className="about-card" aria-labelledby="about-aria-focus">
      <p className="eyebrow">About</p>
      <h2 id="about-aria-focus">Aria Focus</h2>
      <p>Free, open-source, offline focus music.</p>
      <dl>
        <div>
          <dt>Version</dt>
          <dd>0.3.0</dd>
        </div>
        <div>
          <dt>Created by</dt>
          <dd>Aria Zanganeh</dd>
        </div>
        <div>
          <dt>Source</dt>
          <dd>github.com/zanganeh/aria-focus</dd>
        </div>
        <div>
          <dt>Code licence</dt>
          <dd>MIT OR Apache-2.0</dd>
        </div>
      </dl>
      <p className="about-note">
        Your sessions and preferences stay on this device. Aria Focus is not medical treatment.
      </p>
    </section>
  );
}
