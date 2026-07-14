import mark from "../assets/aria-focus-mark.svg";

export function BrandMark({ className = "" }: { className?: string }) {
  return <img className={className} src={mark} alt="" aria-hidden="true" />;
}
