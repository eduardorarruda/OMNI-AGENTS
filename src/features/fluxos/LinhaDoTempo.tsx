import { useEffect, useRef, useState } from "react";
import { lerEventos, type EventoDeFluxo } from "./fluxosService";

const INTERVALO_MS = 1500;
const ROTULO: Record<string, string> = {
  inicio: "início", fim: "fim", falha: "falhou", atividade: "", aprovacao: "aprovação", subfluxo: "subfluxo", aviso: "aviso",
};
const hora = (ms: number) => new Date(ms).toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit", second: "2-digit" });

/** Linha do tempo geral da execução, sempre visível. Pede só o que passou do último `seq` e para
 *  de consultar quando a execução termina (depois de uma última leitura). */
export function LinhaDoTempo({ execucaoId, viva, titulos = {} }: { execucaoId: string; viva: boolean; titulos?: Record<string, string> }) {
  const [eventos, setEventos] = useState<EventoDeFluxo[]>([]);
  const ultimo = useRef(0);
  const fim = useRef<HTMLLIElement>(null);

  useEffect(() => { ultimo.current = 0; setEventos([]); }, [execucaoId]);
  useEffect(() => {
    let ativo = true;
    const ler = async () => {
      try {
        const novos = (await lerEventos(execucaoId, ultimo.current)).filter((e) => e.seq > ultimo.current);
        if (!ativo || novos.length === 0) return;
        ultimo.current = Math.max(...novos.map((e) => e.seq));
        setEventos((atuais) => [...atuais, ...novos]);
      } catch { /* a próxima volta tenta de novo */ }
    };
    void ler();
    const t = viva ? window.setInterval(ler, INTERVALO_MS) : undefined;
    return () => { ativo = false; if (t) window.clearInterval(t); };
  }, [execucaoId, viva]);
  useEffect(() => { if (viva) fim.current?.scrollIntoView?.({ block: "nearest" }); }, [eventos.length, viva]);

  return (
    <section className="fx-tempo" aria-label="Linha do tempo">
      <p className="fx-rotulo">Linha do tempo</p>
      {eventos.length === 0 ? <p className="fx-dica">{viva ? "Começando…" : "Sem eventos."}</p> : (
        <ol className="fx-tempo-lista">
          {eventos.map((e) => (
            <li key={e.seq} className={`fx-tempo-item is-${e.tipo}`}>
              <time>{hora(e.ms)}</time>
              <b>{titulos[e.no] ?? e.no}</b>
              {ROTULO[e.tipo] && <em>{ROTULO[e.tipo]}</em>}
              <span>{e.texto}</span>
            </li>
          ))}
          <li ref={fim} aria-hidden className="fx-h-fim" />
        </ol>
      )}
    </section>
  );
}
