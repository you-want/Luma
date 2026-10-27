import { LoaderCircle } from "lucide-react";

export function ButtonSpinner({ size = 14 }: { size?: number }) {
  return <LoaderCircle size={size} className="button-spinner" aria-hidden="true" />;
}
