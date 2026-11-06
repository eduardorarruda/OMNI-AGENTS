import { useState } from "react";
import { acaoNaExecucao } from "../fluxos/fluxosService";
import { tempoDesde } from "../tarefas/tarefasService";
import { pararConversa } from "../terminal/terminalService";
import { useSegundoPlano, type ItemDeFundo } from "./useSegundoPlano";

const ROTULOS: Record<string, string> = { sessao: "Sessão", subagente: "Subagente", fluxo: "Fluxo" };

interface Props {
  projetoId?: string | null;
  /** Relógio injetável para o "há X min" ser estável nos testes. */
  agora?: number;
  onAbrirConversa: (conversaId: string) => void;
  onAbrirFluxo: (execucaoId: string) => void;
}

const chaveDe = (i: ItemDeFundo) => `${i.tipo}:${i.execucao ?? i.sessao ?? ""}:${i.titulo}:${i.desde_ms}`;

/** Aba da lateral: sessões esperando agente, subagentes e fluxos vivos, cada um com Abrir e Parar. */
export function SegundoPlanoPanel({ projetoId, agora, onAbrirConversa, onAbrirFluxo }: Props) {
  const { itens, erro } = useSegundoPlano(projetoId);
  const [falhas, setFalhas] = useState<Record<string, string>>({});
  const relogio = agora ?? Date.now();

  async function parar(item: ItemDeFundo) {
    const chave = chaveDe(item);
    try {
      if (item.tipo === "fluxo" && item.execucao) await acaoNaExecucao(item.execucao, "cancelar");
      else if (item.sessao) await pararConversa(item.sessao);
      setFalhas((atual) => Object.fromEntries(Object.entries(atual).filter(([k]) => k !== chave)));
    } catch (motivo) {
      setFalhas((atual) => ({ ...atual, [chave]: motivo instanceof Error ? motivo.message : String(motivo) }));
    }
  }

  function abrir(item: ItemDeFundo) {
    if (item.tipo === "fluxo" && item.execucao) onAbrirFluxo(item.execucao);
    else if (item.sessao) onAbrirConversa(item.sessao);
  }

  return (
    <div className="space-y-2 px-1">
      {erro && (
        <p role="alert" className="rounded border border-border-subtle px-2 py-1.5 text-xs text-text-muted">
          Não foi possível consultar o que roda em segundo plano ({erro}). {itens.length > 0 ? "A lista abaixo pode estar desatualizada." : "Tentando de novo."}
        </p>
      )}
      {!erro && itens.length === 0 && (
        <p className="px-2 py-1.5 text-xs text-text-muted">
          Nada rodando em segundo plano. Aqui aparecem subagentes, agentes em segundo plano e fluxos assim que começam.
        </p>
      )}
      <ul className="space-y-1.5">
        {itens.map((item) => {
          const chave = chaveDe(item);
          const subagente = item.tipo === "subagente";
          return (
            <li key={chave} aria-label={item.titulo} className="rounded border border-border-subtle/60 px-2 py-1.5 text-xs">
              <div className="flex items-center justify-between gap-2">
                <span className="truncate font-medium text-text-primary" title={item.titulo}>{item.titulo}</span>
                <span className="shrink-0 text-[10px] uppercase tracking-wider text-text-muted">{ROTULOS[item.tipo] ?? item.tipo}</span>
              </div>
              <p className="truncate text-text-muted" title={item.atividade}>{item.atividade}</p>
              <p className="text-text-muted">{tempoDesde(item.desde_ms, relogio)}</p>
              {subagente && <p className="mt-1 text-[11px] text-text-muted">Parar para a sessão inteira, não só este subagente.</p>}
              <div className="mt-1 flex gap-2">
                <button type="button" className="underline" onClick={() => abrir(item)}>Abrir</button>
                <button type="button" className="underline" onClick={() => void parar(item)}>
                  {subagente ? "Parar a sessão inteira" : "Parar"}
                </button>
              </div>
              {falhas[chave] && <p role="alert" className="mt-1 text-[11px] text-red-400">Não parou: {falhas[chave]}</p>}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
