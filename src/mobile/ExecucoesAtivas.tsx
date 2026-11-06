import { useSyncExternalStore } from "react";
import { criarArmazem } from "../features/fluxos/armazemDeExecucoes";
import { nosAtivos } from "../features/fluxos/execucaoModel";
import type { Execucao } from "../features/fluxos/fluxosService";
import { request } from "./api";

const EM_ANDAMENTO = new Set(["rodando", "aguardando"]);

/** Sem ETag: `GET /fluxos/execucoes` não o devolve, então toda consulta lê a lista inteira (pequena, em loopback). */
const armazem = criarArmazem(async () => {
  const lista = await request<{ id: string; status: string }[]>("/fluxos/execucoes");
  // allSettled: uma execução removida entre a lista e o detalhe não derruba o ciclo.
  const detalhes = await Promise.allSettled(lista.filter((x) => EM_ANDAMENTO.has(x.status))
    .map((x) => request<Execucao>(`/fluxos/execucoes/${encodeURIComponent(x.id)}`)));
  return detalhes.flatMap((d) => (d.status === "fulfilled" ? [d.value] : []));
});

/** Os fluxos rodando agora (a mesma lista do selo do desktop), com o que cada um está fazendo. */
export function ExecucoesAtivas() {
  const { ativas, erro } = useSyncExternalStore(armazem.assinar, armazem.leitura);
  if (ativas.length === 0) return null;
  return (
    <ul className="aprov-fluxo" aria-label="Fluxos em execução">
      {ativas.map((e) => {
        const nos = nosAtivos(e);
        const parado = nos.some((n) => n.status === "aguardando");
        return (
          <li key={e.id} className="aprov-fluxo-item glass" data-tom={parado ? "aprovacao" : "rodando"}>
            <span className="aprov-fluxo-rotulo">{parado ? "Fluxo esperando você" : "Fluxo rodando"}</span>
            <b className="aprov-fluxo-pergunta">{e.fluxo.nome}</b>
            <span className="aprov-fluxo-entrada">{nos.length ? nos.map((n) => n.rotulo).join(", ") : "iniciando"}</span>
          </li>
        );
      })}
      {erro && <li role="status" className="text-xs">Sem atualizar agora: {erro}</li>}
    </ul>
  );
}
