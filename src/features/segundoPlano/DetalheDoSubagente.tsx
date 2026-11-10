import { useEffect, useState } from "react";
import { tempoDesde } from "../tarefas/tarefasService";
import { lerSubagente, pararSubagente, type LeituraDoSubagente } from "./subagenteService";

/** Pedido até este tamanho já aparece aberto; maior começa recolhido para não empurrar o resto. */
const PEDIDO_CURTO = 600;
export const INTERVALO_DO_SUBAGENTE_MS = 3000;
const ESTADOS: Record<string, string> = { rodando: "Rodando", concluido: "Concluído", parado: "Parado" };

interface Props {
  conversaId: string;
  agenteId: string;
  agora?: number;
  onAbrirConversa: (conversaId: string) => void;
}

/** Tudo de um subagente: o que é, o pedido que recebeu, o que fez e o que entregou. */
export function DetalheDoSubagente({ conversaId, agenteId, agora, onAbrirConversa }: Props) {
  const [leitura, setLeitura] = useState<LeituraDoSubagente | null>(null);
  const [erro, setErro] = useState<string | null>(null);
  const [parada, setParada] = useState<"parando" | "na_tela" | "pedido" | null>(null);
  const [falhaAoParar, setFalhaAoParar] = useState<string | null>(null);
  const rodando = !leitura || leitura.detalhe?.estado === "rodando";

  useEffect(() => {
    let vivo = true;
    const ler = () => lerSubagente(conversaId, agenteId)
      .then((nova) => { if (vivo) { setLeitura(nova); setErro(null); } })
      .catch((motivo) => { if (vivo) setErro(motivo instanceof Error ? motivo.message : String(motivo)); });
    // Lê ao abrir; depois, só enquanto roda: terminado ou parado, o transcript não muda mais.
    if (!leitura) void ler();
    const timer = rodando ? setInterval(() => void ler(), INTERVALO_DO_SUBAGENTE_MS) : null;
    return () => { vivo = false; if (timer) clearInterval(timer); };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `leitura` só decide a primeira leitura
  }, [conversaId, agenteId, rodando]);

  async function parar() {
    const d = leitura?.detalhe;
    if (!d) return;
    setParada("parando"); setFalhaAoParar(null);
    try { setParada(await pararSubagente(conversaId, d)); }
    catch (motivo) { setParada(null); setFalhaAoParar(motivo instanceof Error ? motivo.message : String(motivo)); }
  }

  const d = leitura?.detalhe ?? null;
  const relogio = agora ?? Date.now();
  // O pedido já aparece no bloco dele: a primeira fala do usuário na linha do tempo é ele mesmo.
  const passos = (leitura?.mensagens ?? []).filter((m, i) => !(i === 0 && m.role === "user" && d && m.text === d.prompt));

  return (
    <div className="mt-1.5 space-y-2 border-t border-border-subtle/60 pt-1.5" aria-label="Detalhe do subagente">
      {erro && <p role="alert" className="text-[11px] text-red-400">Não foi possível ler o subagente: {erro}</p>}
      {!d && !erro && <p className="text-[11px] text-text-muted">Carregando…</p>}
      {d && (
        <>
          <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[11px] text-text-muted">
            <span className="rounded bg-surface-2 px-1 font-medium text-text-primary">{d.tipo}</span>
            {d.modelo && <span title="Modelo">modelo {d.modelo}</span>}
            <span className={d.estado === "rodando" ? "text-accent" : undefined}>{ESTADOS[d.estado] ?? d.estado}</span>
            {d.desde_ms !== null && <span>começou {tempoDesde(d.desde_ms, relogio)}</span>}
          </div>
          {d.descricao && <p className="text-[11px] text-text-primary">{d.descricao}</p>}
          <details open={d.prompt.length <= PEDIDO_CURTO}>
            <summary className="cursor-pointer text-[11px] font-medium text-text-primary">Pedido recebido</summary>
            {d.prompt
              ? <pre className="mt-1 max-h-64 overflow-auto whitespace-pre-wrap break-words rounded bg-surface-2 p-1.5 font-sans text-[11px] text-text-muted">{d.prompt}</pre>
              : <p className="mt-1 text-[11px] text-text-muted">O pedido não ficou gravado.</p>}
          </details>
          <section>
            <h4 className="text-[11px] font-medium text-text-primary">O que fez</h4>
            {leitura?.cortado && <p className="text-[10px] text-text-muted">Mostrando só as ações mais recentes.</p>}
            {passos.length === 0
              ? <p className="text-[11px] text-text-muted">Nenhuma ação ainda.</p>
              : (
                <ol className="mt-0.5 max-h-72 space-y-0.5 overflow-auto text-[11px]">
                  {passos.map((m) => (
                    <li key={m.id} className="break-words">
                      {m.kind === "tool"
                        ? <><b className="text-text-primary">{m.tool}</b> <span className="text-text-muted" title={m.text}>{m.text}</span></>
                        : <span className="whitespace-pre-wrap text-text-primary">{m.text}</span>}
                    </li>
                  ))}
                </ol>
              )}
          </section>
          {d.resultado && (
            <section>
              <h4 className="text-[11px] font-medium text-text-primary">Resultado</h4>
              <pre className="mt-0.5 max-h-72 overflow-auto whitespace-pre-wrap break-words rounded bg-surface-2 p-1.5 font-sans text-[11px] text-text-primary">{d.resultado}</pre>
            </section>
          )}
        </>
      )}
      <div className="flex flex-wrap gap-2 text-[11px]">
        <button type="button" className="underline" onClick={() => onAbrirConversa(conversaId)}>Abrir conversa</button>
        {d?.estado === "rodando" && (
          <button type="button" className="underline" disabled={parada === "parando"} onClick={() => void parar()}
            title="Encerra só este subagente; os outros e a conversa seguem">
            {parada === "parando" ? "Parando…" : "Parar este subagente"}
          </button>
        )}
      </div>
      {parada === "na_tela" && <p className="text-[11px] text-text-muted">Pedido de parada enviado ao Claude.</p>}
      {parada === "pedido" && <p className="text-[11px] text-text-muted">Pedimos ao agente principal que pare só este subagente.</p>}
      {falhaAoParar && <p role="alert" className="text-[11px] text-red-400">Não parou: {falhaAoParar}</p>}
    </div>
  );
}
