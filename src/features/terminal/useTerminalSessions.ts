import { useEffect, useState } from "react";
import { ritmo } from "../../lib/modoLeve";
import { ensureEngine, listTerminalSessions, type TerminalSession } from "./terminalService";

export function useTerminalSessions() {
  const [sessions, setSessions] = useState<TerminalSession[]>([]);
  const [online, setOnline] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let timer: number | undefined;
    // Lista igual não vira estado: `setSessions` re-renderiza o App inteiro a cada segundo.
    let ultima = "";

    async function refresh() {
      try {
        await ensureEngine();
        const next = await listTerminalSessions();
        if (!cancelled) {
          const texto = JSON.stringify(next);
          if (texto !== ultima) {
            ultima = texto;
            setSessions(next);
          }
          setOnline(true);
        }
      } catch {
        if (!cancelled) setOnline(false);
      }
      if (!cancelled) timer = window.setTimeout(refresh, ritmo(1_000, 3_000));
    }

    void refresh();
    return () => {
      cancelled = true;
      if (timer) window.clearTimeout(timer);
    };
  }, []);

  return { sessions, online };
}
