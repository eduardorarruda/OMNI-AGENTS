import { useSyncExternalStore } from "react";
import { criarArmazem, type LeituraDasExecucoes } from "./armazemDeExecucoes";
import { listarExecucoes, obterExecucao } from "./fluxosService";

const EM_ANDAMENTO = new Set(["rodando", "aguardando"]);

const armazem = criarArmazem(async () => {
  const lista = await listarExecucoes();
  return Promise.all(lista.filter((x) => EM_ANDAMENTO.has(x.status)).map((x) => obterExecucao(x.id)));
});

/** As execuções vivas, com uma consulta a cada 4 s compartilhada por quem usa o hook. */
export function useExecucoesAtivas(): LeituraDasExecucoes {
  return useSyncExternalStore(armazem.assinar, armazem.leitura);
}
