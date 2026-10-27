import { useCallback, useEffect, useRef, useState } from "react";

// Lock immediately, before React renders the disabled state. A second click in
// the same frame must not open another native dialog or send another IPC call.
export function useAsyncAction<Action extends string>() {
  const active = useRef(false);
  const mounted = useRef(true);
  const [pending, setPending] = useState<Action | null>(null);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  const run = useCallback(async (action: Action, task: () => Promise<void>) => {
    if (active.current) return;
    active.current = true;
    setPending(action);
    try { await task(); }
    finally {
      active.current = false;
      if (mounted.current) setPending(null);
    }
  }, []);
  return { pending, run };
}
