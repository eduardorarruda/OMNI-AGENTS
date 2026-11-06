import { useSyncExternalStore } from "react";
import { criarArmazem, type LeituraDasExecucoes } from "./armazemDeExecucoes";
import { listarExecucoes, obterExecucao } from "./fluxosService";

const EM_ANDAMENTO = new Set(["rodando", "aguardando"]);

const armazem = criarArmazem(async () => {
  const lista = await listarExecucoes();
  // allSettled: uma execução removida entre a lista e o detalhe não derruba o ciclo.
  const detalhes = await Promise.allSettled(lista.filter((x) => EM_ANDAMENTO.has(x.status)).map((x) => obterExecucao(x.id)));
  return detalhes.flatMap((d) => (d.status === "fulfilled" ? [d.value] : []));
});

/** As execuções vivas, com uma consulta a cada 4 s compartilhada por quem usa o hook. */
export function useExecucoesAtivas(): LeituraDasExecucoes {
  return useSyncExternalStore(armazem.assinar, armazem.leitura);
}
