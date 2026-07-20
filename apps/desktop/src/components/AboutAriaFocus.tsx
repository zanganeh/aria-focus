import type { MouseEvent } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";

const GITHUB_LINKS = [
  {
    label: "Report a bug",
    href: "https://github.com/zanganeh/aria-focus/issues/new?template=bug_report.yml",
  },
  {
    label: "Request a feature",
    href: "https://github.com/zanganeh/aria-focus/issues/new?template=feature_request.yml",
  },
  { label: "Open issues", href: "https://github.com/zanganeh/aria-focus/issues" },
  { label: "Source & releases", href: "https://github.com/zanganeh/aria-focus/releases" },
] as const;

function openGithubLink(event: MouseEvent<HTMLAnchorElement>, href: string) {
  if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return;
  }
  event.preventDefault();
  void openUrl(href).catch(() => {
    window.open(href, "_blank", "noopener,noreferrer");
  });
}

export function AboutAriaFocus() {
  return (
    <section className="about-card" aria-labelledby="about-aria-focus">
      <p className="eyebrow">About</p>
      <h2 id="about-aria-focus">Aria Focus</h2>
      <p>Free, open-source, offline focus music.</p>
      <dl>
        <div>
          <dt>Version</dt>
          <dd>0.4.0</dd>
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
      <section className="about-help" aria-labelledby="about-help-heading">
        <h3 id="about-help-heading">Help &amp; feedback</h3>
        <p>GitHub opens externally in your browser.</p>
        <nav aria-label="GitHub links">
          {GITHUB_LINKS.map(({ label, href }) => (
            <a
              key={label}
              href={href}
              target="_blank"
              rel="noreferrer"
              onClick={(event) => openGithubLink(event, href)}
            >
              {label}
            </a>
          ))}
        </nav>
      </section>
    </section>
  );
}
