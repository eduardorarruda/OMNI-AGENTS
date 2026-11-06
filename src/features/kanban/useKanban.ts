import { useCallback, useEffect, useRef, useState } from "react";
import type { KanbanAction, KanbanState } from "../../types/kanban";
import {
  AVISO_GRAVACAO_FALHOU,
  aplicarNoEngine,
  carregarKanbanComOrigem,
  lerDoEngine,
} from "../../services/kanbanService";
import { definirAvisoDoKanban } from "./kanbanAviso";
import { emptyKanban, kanbanReducer } from "./kanbanReducer";

const REPETIR_CARGA_MS = 3_000;

/** O reducer continua dono da tela: a mudança aparece na hora e é confirmada no engine, uma operação
 * de um cartão por vez e em ordem. Se o engine recusar, volta ao que ele guarda (ou ao estado de antes)
 * e avisa. Antes da carga inicial, ações são ignoradas para não serem pisadas por ela. */
export function useKanban() {
  const [kanban, setKanban] = useState<KanbanState>(emptyKanban);
  const [carregado, setCarregado] = useState(false);
  const atual = useRef(kanban);
  const fila = useRef<Promise<void>>(Promise.resolve());
  const carregadoRef = useRef(false);

  const trocar = useCallback((estado: KanbanState) => {
    atual.current = estado;
    setKanban(estado);
  }, []);

  useEffect(() => {
    let vivo = true;
    let temporizador: number | undefined;
    async function carregar() {
      const { estado, origem } = await carregarKanbanComOrigem();
      if (!vivo) return;
      // Cópia local só serve para olhar: depois da primeira carga não é reposta por cima do que o usuário fez.
      if (origem === "engine" || !carregadoRef.current) trocar(estado);
      carregadoRef.current = true;
      setCarregado(true);
      // Engine fora do ar na partida: tenta de novo até ele responder.
      if (origem === "local") temporizador = window.setTimeout(() => void carregar(), REPETIR_CARGA_MS);
    }
    void carregar();
    return () => {
      vivo = false;
      window.clearTimeout(temporizador);
    };
  }, [trocar]);

  const dispatch = useCallback(
    (acao: KanbanAction) => {
      if (!carregadoRef.current) return;
      const antes = atual.current;
      const depois = kanbanReducer(antes, acao);
      if (depois === antes) return;
      trocar(depois);
      if (acao.type === "REPLACE_STATE") return;
      fila.current = fila.current.then(async () => {
        try {
          await aplicarNoEngine(antes, depois);
        } catch {
          definirAvisoDoKanban(AVISO_GRAVACAO_FALHOU);
          try {
            trocar(await lerDoEngine());
          } catch {
            trocar(antes);
          }
        }
      });
    },
    [trocar],
  );

  return { kanban, dispatch, carregado };
}
