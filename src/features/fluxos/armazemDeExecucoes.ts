import type { Execucao } from "./fluxosService";

export const INTERVALO_DA_CONSULTA_MS = 4000;

export interface LeituraDasExecucoes {
  /** Rodando ou esperando você (inclui as que aguardam aprovação). */
  ativas: Execucao[];
  /** As que estão paradas esperando você. */
  aguardando: Execucao[];
  /** Mensagem da última consulta que falhou; `null` quando a última deu certo. */
  erro: string | null;
}
const VAZIA: LeituraDasExecucoes = { ativas: [], aguardando: [], erro: null };

/** Devolve as execuções ativas, ou `null` quando nada mudou desde a última leitura (ETag/304). */
export type Leitor = () => Promise<Execucao[] | null>;

const separar = (ativas: Execucao[], erro: string | null): LeituraDasExecucoes =>
  ({ ativas, aguardando: ativas.filter((e) => e.status === "aguardando"), erro });

/**
 * Uma consulta só para todo mundo: o temporizador nasce na primeira assinatura e morre na última,
 * por mais componentes que mostrem as execuções. Se uma consulta falha, a última leitura fica
 * (o selo não some com o fluxo ainda de pé) e o erro é registrado.
 */
export function criarArmazem(leitor: Leitor) {
  const ouvintes = new Set<() => void>();
  let leitura = VAZIA;
  let timer: ReturnType<typeof setInterval> | null = null;
  let lendo = false;
  let geracao = 0;

  const publicar = (nova: LeituraDasExecucoes) => { leitura = nova; ouvintes.forEach((o) => o()); };

  async function ler() {
    if (lendo) return;
    lendo = true;
    const minha = geracao;
    try {
      const ativas = await leitor();
      if (minha !== geracao) return;
      if (ativas === null) { if (leitura.erro !== null) publicar({ ...leitura, erro: null }); return; }
      publicar(separar(ativas, null));
    } catch (motivo) {
      if (minha !== geracao) return;
      publicar({ ...leitura, erro: motivo instanceof Error ? motivo.message : String(motivo) });
    } finally { lendo = false; }
  }

  return {
    leitura: () => leitura,
    assinar(ouvinte: () => void) {
      ouvintes.add(ouvinte);
      if (!timer) { void ler(); timer = setInterval(() => void ler(), INTERVALO_DA_CONSULTA_MS); }
      return () => {
        ouvintes.delete(ouvinte);
        if (ouvintes.size === 0 && timer) { clearInterval(timer); timer = null; geracao += 1; lendo = false; }
      };
    },
  };
}
