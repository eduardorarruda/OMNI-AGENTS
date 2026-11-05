import { useCallback, useEffect, useRef, useState } from "react";
import { Button, Input } from "../../components/ui";
import { colarNaSessao } from "../conversa/colarNaSessao";
import type { TerminalState } from "../terminal/terminalService";
import { useAgora } from "../todo/TodoList";
import {
  atualizarTarefas,
  estadoDasTarefas,
  filtrarTarefas,
  tempoDesde,
  textoDaTarefa,
  type EstadoDasTarefas,
  type TarefaExterna,
} from "./tarefasService";

/** Lista longa trava o painel; quem tem mais que isso usa a busca. */
const MAXIMO_NA_LISTA = 200;
const SEGUNDO_MS = 1000;

export interface SessaoDoAgente {
  id: string;
  state: TerminalState;
}

interface Props {
  /** A aba de agente aberta agora. Sem ela, "Usar nesta conversa" só explica o que falta. */
  sessao: SessaoDoAgente | null;
  onConfigurar: () => void;
  /** O texto já está no campo: o painel sai da frente para a pessoa conferir e apertar Enter. */
  onUsada: () => void;
}

/**
 * Tarefas da API externa. A API pode ser frágil: nada aqui repete sozinho. Abrir só busca com o cache
 * vencido, e o relógio abaixo conta o bloqueio na tela, sem rede.
 */
export function TarefasExternas({ sessao, onConfigurar, onUsada }: Props) {
  const [estado, setEstado] = useState<EstadoDasTarefas | null>(null);
  const [liberaEm, setLiberaEm] = useState(0);
  const [buscando, setBuscando] = useState(false);
  const [erro, setErro] = useState<string | null>(null);
  const [busca, setBusca] = useState("");
  const [aberta, setAberta] = useState<TarefaExterna | null>(null);
  const agora = useAgora(SEGUNDO_MS);
  // O StrictMode monta duas vezes: sem isto, a abertura pediria duas buscas.
  const abriu = useRef(false);

  const receber = useCallback((novo: EstadoDasTarefas) => {
    setEstado(novo);
    setLiberaEm(Date.now() + novo.espera_ms);
  }, []);

  const buscar = useCallback(async () => {
    setBuscando(true); setErro(null);
    try { receber(await atualizarTarefas()); }
    catch (motivo) {
      setErro(String(motivo));
      await estadoDasTarefas().then(receber).catch(() => undefined);
    } finally { setBuscando(false); }
  }, [receber]);

  useEffect(() => {
    if (abriu.current) return;
    abriu.current = true;
    void estadoDasTarefas().then((atual) => {
      receber(atual);
      if (atual.deve_buscar && atual.espera_ms === 0) void buscar();
    }).catch((motivo) => setErro(String(motivo)));
  }, [buscar, receber]);

  if (!estado) return <p className="tarefas-vazio">{erro ?? "Carregando…"}</p>;
  if (!estado.configurada) return <SemConfiguracao onConfigurar={onConfigurar} />;
  if (aberta) return <DetalheDaTarefa tarefa={aberta} sessao={sessao} onVoltar={() => setAberta(null)} onUsada={onUsada} />;

  const espera = Math.max(0, Math.ceil((liberaEm - Date.now()) / SEGUNDO_MS));
  const achadas = filtrarTarefas(estado.tarefas, busca);
  return (
    <div className="tarefas-corpo min-h-0 gap-2 p-3">
      <div className="flex items-center gap-2">
        <p className="min-w-0 flex-1 truncate text-[11px] text-text-muted">
          {estado.atualizado_em_ms ? `${estado.tarefas.length} tarefas · atualizado ${tempoDesde(estado.atualizado_em_ms, agora)}` : "Ainda não buscadas"}
        </p>
        <Button size="sm" disabled={buscando || espera > 0} onClick={() => void buscar()}>
          {buscando ? "Buscando…" : espera > 0 ? `Aguarde ${espera} s` : "Atualizar"}
        </Button>
      </div>
      {erro && <p role="alert" className="border-l-2 border-danger px-2 py-1 text-[11px] text-danger">{erro}</p>}
      <Input size="sm" type="search" value={busca} onChange={(evento) => setBusca(evento.target.value)}
        placeholder="Buscar por id, título, status ou responsável…" aria-label="Buscar nas tarefas externas" />
      {achadas.length === 0 && <p className="tarefas-vazio">{estado.tarefas.length === 0 ? "Nenhuma tarefa no cache." : "Nada encontrado."}</p>}
      <ul className="min-h-0 flex-1 space-y-1 overflow-y-auto" aria-label="Tarefas externas">
        {achadas.slice(0, MAXIMO_NA_LISTA).map((tarefa) => (
          <li key={tarefa.id}>
            <button type="button" onClick={() => setAberta(tarefa)} className="w-full border border-border-subtle px-3 py-2 text-left hover:border-accent">
              <span className="flex items-baseline gap-2">
                <span className="shrink-0 text-[11px] text-text-muted">#{tarefa.id}</span>
                <span className="truncate text-xs text-text-primary">{tarefa.titulo}</span>
              </span>
              <span className="block truncate text-[11px] text-text-muted">{[tarefa.status, tarefa.responsavel].filter(Boolean).join(" · ")}</span>
            </button>
          </li>
        ))}
      </ul>
      {achadas.length > MAXIMO_NA_LISTA && <p className="text-[11px] text-text-muted">Mostrando {MAXIMO_NA_LISTA} de {achadas.length}. Refine a busca.</p>}
    </div>
  );
}

function SemConfiguracao({ onConfigurar }: { onConfigurar: () => void }) {
  return (
    <div className="tarefas-vazio">
      <p>Ligue sua API de tarefas para ver aqui o que é seu e levar uma tarefa para a conversa com o agente.</p>
      <Button size="sm" className="mt-3" onClick={onConfigurar}>Abrir Configurações</Button>
    </div>
  );
}

function motivoParaNaoColar(sessao: SessaoDoAgente | null): string | null {
  if (!sessao) return "Nenhuma conversa de agente aberta. Abra a aba do agente e tente de novo.";
  // Com o menu de aprovação na tela, o texto colado cairia nas opções dele.
  if (sessao.state === "approval_required") return "O agente está esperando uma aprovação. Responda antes de colar a tarefa.";
  return null;
}

function DetalheDaTarefa({ tarefa, sessao, onVoltar, onUsada }: { tarefa: TarefaExterna; sessao: SessaoDoAgente | null; onVoltar: () => void; onUsada: () => void }) {
  const [aviso, setAviso] = useState<string | null>(null);
  const [colando, setColando] = useState(false);

  async function usar() {
    const motivo = motivoParaNaoColar(sessao);
    if (motivo || !sessao) { setAviso(motivo); return; }
    setAviso(null); setColando(true);
    try {
      await colarNaSessao(sessao.id, await textoDaTarefa(tarefa.id));
      onUsada();
    } catch (falha) { setAviso(String(falha)); }
    finally { setColando(false); }
  }

  return (
    <article className="tarefas-corpo min-h-0 gap-2 p-3" aria-label={`Tarefa ${tarefa.id}`}>
      <div><Button size="sm" variant="ghost" onClick={onVoltar}>Voltar</Button></div>
      <div className="min-h-0 flex-1 space-y-2 overflow-y-auto">
        <h3 className="text-xs text-text-primary">#{tarefa.id} · {tarefa.titulo}</h3>
        <p className="text-[11px] text-text-muted">{[tarefa.status, tarefa.responsavel].filter(Boolean).join(" · ")}</p>
        {tarefa.descricao
          ? <p className="whitespace-pre-wrap break-words text-xs text-text-secondary">{tarefa.descricao}</p>
          : <p className="text-[11px] text-text-muted">Sem descrição.</p>}
        {tarefa.comentarios.length > 0 && (
          <>
            <h4 className="text-[10px] font-medium uppercase tracking-wider text-text-muted">{tarefa.comentarios.length} comentários</h4>
            <ul className="space-y-1" aria-label="Comentários">
              {tarefa.comentarios.map((texto, indice) => (
                <li key={indice} className="whitespace-pre-wrap break-words border border-border-subtle p-2 text-xs text-text-secondary">{texto}</li>
              ))}
            </ul>
          </>
        )}
      </div>
      {aviso && <p role="alert" className="border-l-2 border-danger px-2 py-1 text-[11px] text-danger">{aviso}</p>}
      <Button size="sm" disabled={colando} onClick={() => void usar()}>Usar nesta conversa</Button>
      <p className="text-[11px] text-text-muted">O texto vai para o campo da conversa e não é enviado: confira e aperte Enter.</p>
    </article>
  );
}
