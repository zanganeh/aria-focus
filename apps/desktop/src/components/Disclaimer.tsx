import { PRODUCT_COPY } from "../lib/copy";

/**
 * Non-medical product framing. Must never claim treatment, diagnosis, cure, or
 * that the DSP reproduces any named third-party product.
 */
export function Disclaimer() {
  return (
    <aside className="disclaimer">
      <p>{PRODUCT_COPY.disclaimer}</p>
    </aside>
  );
}
