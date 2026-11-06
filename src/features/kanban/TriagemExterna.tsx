import { useEffect, useState } from "react";
import { Button } from "../../components/ui";
import {
  atualizarExternas,
  escolherProjetoDaExterna,
  lerDoEngine,
  listarProjetosDoEngine,
  type ProjetoDoEngine,
} from "../../services/kanbanService";
import type { KanbanAction, KanbanExterna, KanbanState, ResumoDaImportacao } from "../../types/kanban";

interface TriagemExternaProps {
  /** Projeto aberto no quadro: já vem escolhido nos seletores. */
  projectId: string;
  kanban: KanbanState;
  dispatch: React.Dispatch<KanbanAction>;
}

function textoDoResumo(resumo: ResumoDaImportacao): string {
  return `${resumo.novas} novas, ${resumo.atualizadas} atualizadas, ${resumo.jaNoQuadro} já no quadro, ${resumo.sumiram} sumiram.`;
}

/** A API externa pode ser um servidor frágil: a busca só nasce do clique em Atualizar, e nenhum
 * temporizador chama `atualizarExternas`. */
export function TriagemExterna({ projectId, kanban, dispatch }: TriagemExternaProps) {
  const triagem = kanban.triagem ?? [];
  const [buscando, setBuscando] = useState(false);
  const [mensagem, setMensagem] = useState<string | null>(null);
  const [erro, setErro] = useState<string | null>(null);
  const [projetos, setProjetos] = useState<ProjetoDoEngine[]>([]);
  const temTriagem = triagem.length > 0;

  useEffect(() => {
    if (!temTriagem) return;
    let vivo = true;
    listarProjetosDoEngine()
      .then((lista) => {
        if (vivo) setProjetos(lista);
      })
      .catch(() => {
        if (vivo) setErro("Não foi possível listar os projetos. Dá para mandar só para o projeto aberto.");
      });
    return () => {
      vivo = false;
    };
  }, [temTriagem]);

  async function reler() {
    dispatch({ type: "REPLACE_STATE", state: await lerDoEngine() });
  }

  async function atualizar() {
    setBuscando(true);
    setMensagem(null);
    setErro(null);
    try {
      const resumo = await atualizarExternas();
      await reler();
      setMensagem(textoDoResumo(resumo));
    } catch (falha) {
      setErro(falha instanceof Error ? falha.message : String(falha));
    } finally {
      setBuscando(false);
    }
  }

  async function escolher(tarefa: KanbanExterna, escolhido: string) {
    setErro(null);
    try {
      await escolherProjetoDaExterna(tarefa, escolhido);
      await reler();
    } catch {
      setErro(`Não foi possível mandar "${tarefa.titulo || tarefa.id}" para o Backlog.`);
    }
  }

  return (
    <div className="shrink-0 border-b-2 border-border-subtle bg-bg-surface/60 px-4 py-2 text-xs">
      <div className="flex flex-wrap items-center gap-3">
        <span className="text-[10px] uppercase tracking-wider text-text-muted">Externas ({triagem.length})</span>
        <Button size="sm" variant="glass" disabled={buscando} onClick={() => void atualizar()} title="Busca as tarefas na API configurada em Tarefas. Só busca quando você clica.">
          {buscando ? "Buscando…" : "Atualizar"}
        </Button>
        {mensagem && <span className="text-text-muted">{mensagem}</span>}
        {erro && (
          <span role="alert" className="text-danger">
            {erro}
          </span>
        )}
      </div>
      {temTriagem && (
        <ul className="mt-2 max-h-48 space-y-1.5 overflow-auto">
          {triagem.map((tarefa) => (
            <CartaoDaTriagem
              key={`${tarefa.origem}/${tarefa.id}`}
              tarefa={tarefa}
              projetos={projetos}
              projetoAberto={projectId}
              onEscolher={(escolhido) => void escolher(tarefa, escolhido)}
            />
          ))}
        </ul>
      )}
    </div>
  );
}

function CartaoDaTriagem({
  tarefa,
  projetos,
  projetoAberto,
  onEscolher,
}: {
  tarefa: KanbanExterna;
  projetos: ProjetoDoEngine[];
  projetoAberto: string;
  onEscolher: (projectId: string) => void;
}) {
  const [escolhido, setEscolhido] = useState(projetoAberto);
  const titulo = tarefa.titulo || `Tarefa ${tarefa.id}`;
  const opcoes = projetos.some((projeto) => projeto.id === projetoAberto)
    ? projetos
    : [{ id: projetoAberto, name: "Projeto aberto" }, ...projetos];

  return (
    <li className="flex flex-wrap items-center gap-2 border border-border-subtle bg-bg-elevated p-2">
      <SeloExterna />
      {tarefa.sumiu && <SeloSumiu />}
      <div className="min-w-0 flex-1">
        <p className="truncate font-medium text-text-primary">{titulo}</p>
        {tarefa.descricao && <p className="line-clamp-2 text-text-muted">{tarefa.descricao}</p>}
      </div>
      <select
        aria-label={`Projeto de ${titulo}`}
        value={escolhido}
        onChange={(event) => setEscolhido(event.target.value)}
        className="max-w-48 border border-border-subtle bg-bg-surface px-1.5 py-1 text-xs text-text-primary"
      >
        {opcoes.map((projeto) => (
          <option key={projeto.id} value={projeto.id}>
            {projeto.name}
          </option>
        ))}
      </select>
      <Button size="sm" variant="glass" onClick={() => onEscolher(escolhido)}>
        Mandar para o Backlog
      </Button>
    </li>
  );
}

export function SeloExterna() {
  return <span className="shrink-0 border border-accent px-1 text-[10px] text-accent">Externa</span>;
}

export function SeloSumiu() {
  return (
    <span className="shrink-0 border border-danger px-1 text-[10px] text-danger" title="A origem não devolve mais esta tarefa.">
      Sumiu da origem
    </span>
  );
}
