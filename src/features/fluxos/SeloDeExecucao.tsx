import { useState } from "react";
import { nosAtivos } from "./execucaoModel";
import type { Execucao } from "./fluxosService";
import { pedirExecucao } from "./pedidoDeExecucao";
import { useExecucoesAtivas } from "./useExecucoesAtivas";

/** "Fluxo X: etapa Y" (ou "aprovar Y" quando está parado esperando você). */
export function textoDaExecucao(e: Execucao): string {
  const ativos = nosAtivos(e);
  const parado = ativos.find((n) => n.status === "aguardando");
  if (parado) return `Fluxo ${e.fluxo.nome}: aprovar ${parado.rotulo}`;
  return `Fluxo ${e.fluxo.nome}: ${ativos[0]?.rotulo ?? "iniciando"}`;
}

/** Selo na barra do topo: mostra o fluxo rodando em qualquer tela e leva até a execução. */
export function SeloDeExecucao({ onAbrir }: { onAbrir: () => void }) {
  const { ativas, aguardando, erro } = useExecucoesAtivas();
  const [aberto, setAberto] = useState(false);
  if (ativas.length === 0) return null;

  const abrir = (id: string) => { pedirExecucao(id); setAberto(false); onAbrir(); };
  const tom = aguardando.length > 0 ? "aprovacao" : "rodando";
  const varias = ativas.length > 1;
  const texto = varias
    ? `${ativas.length} fluxos rodando${aguardando.length ? ` · ${aguardando.length} esperando você` : ""}`
    : textoDaExecucao(ativas[0]);
  const aviso = erro ? ` (sem atualizar: ${erro})` : "";

  return (
    <span className="selo-execucao-caixa">
      <button type="button" className="titlebar-acao selo-execucao" data-tom={tom} data-erro={erro ? "sim" : undefined}
        title={`${texto}${aviso}`} aria-expanded={varias ? aberto : undefined}
        onClick={() => (varias ? setAberto(!aberto) : abrir(ativas[0].id))}>
        <span aria-hidden className="selo-execucao-ponto" />
        {texto}
      </button>
      {varias && aberto && (
        <ul className="selo-execucao-lista glass" aria-label="Fluxos em execução">
          {ativas.map((e) => (
            <li key={e.id}><button type="button" onClick={() => abrir(e.id)}>{textoDaExecucao(e)}</button></li>
          ))}
        </ul>
      )}
    </span>
  );
}
