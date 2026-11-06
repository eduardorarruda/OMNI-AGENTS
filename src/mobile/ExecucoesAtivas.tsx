import { useSyncExternalStore } from "react";
import { criarArmazem } from "../features/fluxos/armazemDeExecucoes";
import { nosAtivos } from "../features/fluxos/execucaoModel";
import type { Execucao } from "../features/fluxos/fluxosService";
import { request, requestSeMudou } from "./api";

const EM_ANDAMENTO = new Set(["rodando", "aguardando"]);

/** A lista usa ETag: sem mudança vem 304 (`null`) e a tela fica como está. */
const armazem = criarArmazem(async () => {
  const lista = await requestSeMudou<{ id: string; status: string }[]>("/fluxos/execucoes");
  if (lista === null) return null;
  return Promise.all(lista.filter((x) => EM_ANDAMENTO.has(x.status))
    .map((x) => request<Execucao>(`/fluxos/execucoes/${encodeURIComponent(x.id)}`)));
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
