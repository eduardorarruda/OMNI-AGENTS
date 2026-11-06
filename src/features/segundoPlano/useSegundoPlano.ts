import { useCallback, useSyncExternalStore } from "react";
import { engineApi } from "../terminal/terminalService";

export interface ItemDeFundo {
  tipo: "sessao" | "subagente" | "fluxo" | string;
  titulo: string;
  atividade: string;
  /** `null`: a fonte não sabe quando começou. */
  desde_ms: number | null;
  projeto_id: string;
  /** Conversa do OMNI a abrir ou parar. */
  sessao: string | null;
  /** Execução de fluxo. */
  execucao: string | null;
}

export interface LeituraDoSegundoPlano { itens: ItemDeFundo[]; erro: string | null }

export const INTERVALO_SEGUNDO_PLANO_MS = 4000;
const VAZIA: LeituraDoSegundoPlano = { itens: [], erro: null };

export async function lerSegundoPlano(projetoId?: string | null): Promise<ItemDeFundo[]> {
  const consulta = projetoId ? `?projeto=${encodeURIComponent(projetoId)}` : "";
  const r = await engineApi("GET", `/segundo-plano${consulta}`, null);
  const dados = r.body ? JSON.parse(r.body) : null;
  if (r.status >= 400) throw new Error(dados?.error ?? `Falha HTTP ${r.status}`);
  return Array.isArray(dados?.itens) ? dados.itens : [];
}

interface Armazem {
  leitura: LeituraDoSegundoPlano;
  ouvintes: Set<() => void>;
  timer: ReturnType<typeof setInterval> | null;
  lendo: boolean;
}
// Uma consulta por projeto, compartilhada por quem mostra (aba da lateral, painel, faixa).
const armazens = new Map<string, Armazem>();

async function atualizar(chave: string, projetoId?: string | null) {
  const a = armazens.get(chave);
  if (!a || a.lendo) return;
  a.lendo = true;
  try {
    a.leitura = { itens: await lerSegundoPlano(projetoId), erro: null };
  } catch (motivo) {
    // Mantém a última lista: o aviso diz que está desatualizada, em vez de fingir que não há nada.
    a.leitura = { itens: a.leitura.itens, erro: motivo instanceof Error ? motivo.message : String(motivo) };
  } finally {
    a.lendo = false;
    if (armazens.get(chave) === a) a.ouvintes.forEach((o) => o());
  }
}

function assinar(chave: string, projetoId: string | undefined, ouvinte: () => void) {
  let a = armazens.get(chave);
  if (!a) { a = { leitura: VAZIA, ouvintes: new Set(), timer: null, lendo: false }; armazens.set(chave, a); }
  a.ouvintes.add(ouvinte);
  if (!a.timer) {
    void atualizar(chave, projetoId);
    a.timer = setInterval(() => void atualizar(chave, projetoId), INTERVALO_SEGUNDO_PLANO_MS);
  }
  return () => {
    a.ouvintes.delete(ouvinte);
    if (a.ouvintes.size === 0) { if (a.timer) clearInterval(a.timer); armazens.delete(chave); }
  };
}

/** O que roda em segundo plano (no projeto, se informado), consultado a cada 4 s. */
export function useSegundoPlano(projetoId?: string | null): LeituraDoSegundoPlano {
  const chave = projetoId ?? "";
  // Estável por projeto: uma função nova a cada render faria o React reassinar, apagar o armazém
  // e consultar de novo sem esperar os 4 s.
  const inscrever = useCallback((ouvinte: () => void) => assinar(chave, chave || undefined, ouvinte), [chave]);
  const ler = useCallback(() => armazens.get(chave)?.leitura ?? VAZIA, [chave]);
  return useSyncExternalStore(inscrever, ler);
}
