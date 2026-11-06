import { useCallback, useEffect, useRef, useState } from "react";
import type { KanbanAction, KanbanState } from "../../types/kanban";
import {
  AVISO_GRAVACAO_FALHOU,
  aplicarNoEngine,
  carregarKanbanComOrigem,
  lerDoEngine,
  type OrigemDoKanban,
} from "../../services/kanbanService";
import { definirAvisoDoKanban } from "./kanbanAviso";
import { emptyKanban, kanbanReducer } from "./kanbanReducer";

const REPETIR_CARGA_MS = 3_000;
/** Só o `GET /kanban` local: agentes mudam cartões pelo MCP. A busca externa não passa por aqui. */
const RELER_MS = 3_000;

interface Operacao {
  antes: KanbanState;
  depois: KanbanState;
}

/** O reducer continua dono da tela: a mudança aparece na hora e é confirmada no engine, uma operação
 * de um cartão por vez e em ordem. Se uma gravação falhar, o resto da fila é descartado (ele foi montado
 * em cima de um estado que não existe no engine), o engine é relido uma vez e o aviso diz que as mudanças
 * não gravadas foram desfeitas. Enquanto isso, e antes da carga inicial, ações novas são ignoradas.
 * Com o quadro vindo só da cópia local (engine fora, ou migração que falhou), nada é gravado. */
export function useKanban() {
  const [kanban, setKanban] = useState<KanbanState>(emptyKanban);
  const atual = useRef(kanban);
  const pendentes = useRef<Operacao[]>([]);
  const drenando = useRef(false);
  const aceitando = useRef(false);
  const origem = useRef<OrigemDoKanban>("engine");
  /** Sobe a cada gesto: leitura que saiu antes dele veio velha e é descartada. */
  const gestos = useRef(0);
  const lendo = useRef(false);

  const trocar = useCallback((estado: KanbanState) => {
    atual.current = estado;
    setKanban(estado);
  }, []);

  useEffect(() => {
    let vivo = true;
    let temporizador: number | undefined;
    async function carregar() {
      if (drenando.current) {
        temporizador = window.setTimeout(() => void carregar(), REPETIR_CARGA_MS);
        return;
      }
      const resultado = await carregarKanbanComOrigem();
      if (!vivo) return;
      origem.current = resultado.origem;
      // Cópia local só serve para olhar: depois da primeira carga não é reposta por cima do que está na tela.
      if (resultado.origem === "engine" || !aceitando.current) trocar(resultado.estado);
      aceitando.current = true;
      // Engine fora do ar (ou migração caída): tenta de novo até dar certo.
      if (resultado.origem === "local") temporizador = window.setTimeout(() => void carregar(), REPETIR_CARGA_MS);
    }
    void carregar();
    return () => {
      vivo = false;
      window.clearTimeout(temporizador);
    };
  }, [trocar]);

  useEffect(() => {
    let vivo = true;
    const ocupado = () => drenando.current || pendentes.current.length > 0;
    async function reler() {
      if (lendo.current || !aceitando.current || origem.current !== "engine" || ocupado()) return;
      lendo.current = true;
      const gesto = gestos.current;
      const estado = await lerDoEngine().catch(() => null);
      lendo.current = false;
      if (!vivo || !estado || gesto !== gestos.current || ocupado()) return;
      if (JSON.stringify(estado) !== JSON.stringify(atual.current)) trocar(estado);
    }
    const temporizador = window.setInterval(() => void reler(), RELER_MS);
    return () => {
      vivo = false;
      window.clearInterval(temporizador);
    };
  }, [trocar]);

  const drenar = useCallback(async () => {
    if (drenando.current) return;
    drenando.current = true;
    let falhou = false;
    while (pendentes.current.length > 0) {
      const operacao = pendentes.current.shift()!;
      if (falhou) continue;
      try {
        if (origem.current === "local") throw new Error("Kanban sem engine para gravar");
        await aplicarNoEngine(operacao.antes, operacao.depois);
      } catch {
        falhou = true;
        aceitando.current = false;
      }
    }
    if (falhou) {
      const resultado = await carregarKanbanComOrigem();
      origem.current = resultado.origem;
      trocar(resultado.estado);
      if (resultado.origem === "engine") definirAvisoDoKanban(AVISO_GRAVACAO_FALHOU);
      aceitando.current = true;
    }
    drenando.current = false;
  }, [trocar]);

  const dispatch = useCallback(
    (acao: KanbanAction) => {
      if (!aceitando.current) return;
      gestos.current += 1;
      const antes = atual.current;
      const depois = kanbanReducer(antes, acao);
      if (depois === antes) return;
      trocar(depois);
      if (acao.type === "REPLACE_STATE" || acao.type === "TASK_STARTED") return;
      pendentes.current.push({ antes, depois });
      void drenar();
    },
    [drenar, trocar],
  );

  return { kanban, dispatch };
}
