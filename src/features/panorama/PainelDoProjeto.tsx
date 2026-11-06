import { useMemo } from "react";
import { Background, ReactFlow, ReactFlowProvider, type Edge, type Node } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { nosAtivos } from "../fluxos/execucaoModel";
import type { Execucao } from "../fluxos/fluxosService";
import { useExecucoesAtivas } from "../fluxos/useExecucoesAtivas";
import { useSegundoPlano } from "../segundoPlano/useSegundoPlano";
import { tempoDesde } from "../tarefas/tarefasService";
import "./painelDoProjeto.css";

const ROTULO_TIPO: Record<string, string> = { sessao: "Sessão", subagente: "Subagente", fluxo: "Fluxo" };

/** Nós e arestas do mini-grafo: o nó ativo aceso, os concluídos esmaecidos. Só leitura. */
export function montarMiniGrafo(execucao: Execucao): { nodes: Node[]; edges: Edge[] } {
  const ativos = new Set(nosAtivos(execucao).map((n) => n.id));
  const nodes: Node[] = execucao.fluxo.nos.map((no) => {
    const estado = execucao.nos[no.id]?.status;
    const classe = ativos.has(no.id) ? "is-ativo" : estado === "concluido" ? "is-feito" : estado === "falhou" ? "is-falhou" : "";
    return { id: no.id, position: { x: no.x, y: no.y }, data: { label: no.titulo || no.id }, className: `mg-no ${classe}`.trim(), draggable: false, selectable: false, connectable: false };
  });
  const edges: Edge[] = execucao.fluxo.conexoes.map((c, i) => ({ id: `${c.de}-${c.para}-${i}`, source: c.de, target: c.para, selectable: false }));
  return { nodes, edges };
}

function MiniGrafo({ execucao }: { execucao: Execucao }) {
  const { nodes, edges } = useMemo(() => montarMiniGrafo(execucao), [execucao]);
  return (
    <div className="mg-grafo" role="img" aria-label={`Esquema do fluxo ${execucao.fluxo.nome}`}>
      <ReactFlowProvider>
        <ReactFlow nodes={nodes} edges={edges} fitView proOptions={{ hideAttribution: true }}
          nodesDraggable={false} nodesConnectable={false} elementsSelectable={false} panOnDrag={false}
          zoomOnScroll={false} zoomOnPinch={false} zoomOnDoubleClick={false} preventScrolling={false}>
          <Background />
        </ReactFlow>
      </ReactFlowProvider>
    </div>
  );
}

/** Aba "execucao-projeto": cada fluxo vivo do projeto como esquema, mais as tarefas em segundo plano. */
export function PainelDoProjeto({ projetoId }: { projetoId: string }) {
  const { ativas } = useExecucoesAtivas();
  const { itens, erro } = useSegundoPlano(projetoId);
  const fluxos = ativas.filter((e) => e.projeto_id === projetoId);
  const tarefas = itens.filter((i) => i.projeto_id === projetoId && i.tipo !== "fluxo");
  const agora = Date.now();

  return (
    <div className="mg-painel">
      {erro && <p role="alert" className="mg-aviso">Não foi possível atualizar o segundo plano ({erro}). O que aparece pode estar desatualizado.</p>}
      {fluxos.length === 0 && tarefas.length === 0 && (
        <p className="mg-vazio">Nada rodando neste projeto agora. Quando um fluxo ou um agente em segundo plano começar, ele aparece aqui.</p>
      )}
      {fluxos.map((e) => (
        <section key={e.id} className="mg-fluxo" aria-label={`Fluxo ${e.fluxo.nome}`}>
          <h2>{e.fluxo.nome}</h2>
          <MiniGrafo execucao={e} />
          <ul className="mg-ativos">
            {nosAtivos(e).map((n) => (
              <li key={n.id}>
                <strong>{n.rotulo}</strong>
                <span>{n.status === "aguardando" ? "esperando você" : n.atividade || "trabalhando"}</span>
              </li>
            ))}
          </ul>
        </section>
      ))}
      {tarefas.length > 0 && (
        <section className="mg-tarefas" aria-label="Tarefas em segundo plano">
          <h2>Em segundo plano</h2>
          <ul>
            {tarefas.map((t) => (
              <li key={`${t.tipo}:${t.sessao}:${t.titulo}:${t.desde_ms}`}>
                <strong>{t.titulo}</strong>
                <span className="mg-tipo">{ROTULO_TIPO[t.tipo] ?? t.tipo}</span>
                <span>{t.atividade}</span>
                <span>{tempoDesde(t.desde_ms, agora)}</span>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
